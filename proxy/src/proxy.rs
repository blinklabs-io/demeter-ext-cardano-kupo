use async_trait::async_trait;
use bytes::Bytes;
use pingora::http::{ResponseHeader, StatusCode};
use pingora::Result;
use pingora::{
    proxy::{ProxyHttp, Session},
    upstreams::peer::HttpPeer,
};
use pingora_limits::rate::Rate;
use regex::Regex;
use std::sync::Arc;
use std::time::Instant;
use tracing::info;

use crate::config::Config;
use crate::{Consumer, State, Tier};

static DMTR_API_KEY: &str = "dmtr-api-key";

pub struct KupoProxy {
    state: Arc<State>,
    config: Arc<Config>,
    host_regex: Regex,
    private_endpoint_regex: Regex,
}
impl KupoProxy {
    pub fn new(state: Arc<State>, config: Arc<Config>) -> Self {
        let host_regex = Regex::new(r"([dmtr_]?[\w\d-]+)?\.?.+").unwrap();
        let private_endpoint_regex = Regex::new(&config.private_endpoint).unwrap();

        Self {
            state,
            config,
            host_regex,
            private_endpoint_regex,
        }
    }

    async fn has_limiter(&self, consumer: &Consumer) -> bool {
        let rate_limiter_map = self.state.limiter.read().await;
        rate_limiter_map.get(&consumer.key).is_some()
    }

    async fn add_limiter(&self, consumer: &Consumer, tier: &Tier) {
        let rates = tier
            .rates
            .iter()
            .map(|r| (r.clone(), Rate::new(r.interval)))
            .collect();

        self.state
            .limiter
            .write()
            .await
            .insert(consumer.key.clone(), rates);
    }

    async fn limiter(&self, consumer: &Consumer) -> Result<bool> {
        let tiers = self.state.tiers.read().await.clone();
        let tier = tiers.get(&consumer.tier);
        if tier.is_none() {
            return Ok(true);
        }
        let tier = tier.unwrap();

        if !self.has_limiter(consumer).await {
            self.add_limiter(consumer, tier).await;
        }

        let rate_limiter_map = self.state.limiter.read().await;
        let rates = rate_limiter_map.get(&consumer.key).unwrap();

        if rates
            .iter()
            .any(|(t, r)| r.observe(&consumer.key, 1) > t.limit)
        {
            return Ok(true);
        }

        Ok(false)
    }

    async fn respond_health(&self, session: &mut Session, ctx: &mut Context) {
        ctx.is_health_request = true;
        session.set_keepalive(None);

        let is_healthy = *self.state.upstream_health.read().await;
        let (code, message) = if is_healthy {
            (200, "OK")
        } else {
            (500, "UNHEALTHY")
        };

        let header = Box::new(ResponseHeader::build(code, None).unwrap());
        session.write_response_header(header, true).await.unwrap();
        session
            .write_response_body(Some(Bytes::from(message)), true)
            .await
            .unwrap();
    }

    async fn respond_unauthorized(&self, session: &mut Session) {
        session.set_keepalive(None);

        let header = Box::new(ResponseHeader::build(StatusCode::UNAUTHORIZED, None).unwrap());
        session.write_response_header(header, true).await.unwrap();
        session
            .write_response_body(
                Some(Bytes::from("unauthorized to request the endpoint")),
                true,
            )
            .await
            .unwrap();
    }
}

#[derive(Debug, Default)]
pub struct Context {
    is_health_request: bool,
    instance: String,
    consumer: Consumer,
    start_time: Option<Instant>,
}

#[async_trait]
impl ProxyHttp for KupoProxy {
    type CTX = Context;
    fn new_ctx(&self) -> Self::CTX {
        Context::default()
    }

    async fn request_filter(&self, session: &mut Session, ctx: &mut Self::CTX) -> Result<bool>
    where
        Self::CTX: Send + Sync,
    {
        ctx.start_time = Some(Instant::now());
        let state = self.state.clone();

        // Check if the request is going to the health endpoint before continuing.
        let path = session.req_header().uri.path();
        if path == self.config.health_endpoint {
            self.respond_health(session, ctx).await;
            return Ok(true);
        }

        let pattern = format!("{}{path}", &session.req_header().method);
        if self.private_endpoint_regex.is_match(&pattern) {
            self.respond_unauthorized(session).await;
            return Ok(true);
        }

        // Extract key from host or header.
        let host = session
            .get_header("host")
            .map(|v| v.to_str().unwrap())
            .unwrap();
        let captures = self.host_regex.captures(host).unwrap();
        let key = session
            .get_header(DMTR_API_KEY)
            .and_then(|v| v.to_str().ok())
            .or_else(|| captures.get(1).map(|v| v.as_str()))
            .unwrap_or_default();

        let Some(consumer) = state.get_consumer(key).await else {
            session.respond_error(401).await?;
            return Ok(true);
        };

        if consumer.network != self.config.network {
            session.respond_error(401).await?;
            return Ok(true);
        }

        ctx.consumer = consumer;
        ctx.instance = self.config.instance(ctx.consumer.pruned);

        if self.limiter(&ctx.consumer).await? {
            session.respond_error(429).await?;
            return Ok(true);
        }

        Ok(false)
    }

    async fn upstream_peer(
        &self,
        _session: &mut Session,
        ctx: &mut Self::CTX,
    ) -> Result<Box<HttpPeer>> {
        let http_peer = HttpPeer::new(&ctx.instance, false, String::default());
        Ok(Box::new(http_peer))
    }

    async fn logging(
        &self,
        session: &mut Session,
        _e: Option<&pingora::Error>,
        ctx: &mut Self::CTX,
    ) {
        if !ctx.is_health_request {
            let response_code = session
                .response_written()
                .map_or(0, |resp| resp.status.as_u16());

            self.state.metrics.inc_http_total_request(
                &ctx.consumer,
                &self.config.proxy_namespace,
                &ctx.instance,
                &response_code,
            );

            if let Some(start) = ctx.start_time {
                let dur = start.elapsed();

                self.state.metrics.observe_http_request_duration(
                    &ctx.consumer,
                    &response_code,
                    dur,
                );
                info!(
                    response_time = dur.as_millis(),
                    "{} response code: {response_code}",
                    self.request_summary(session, ctx)
                );
            } else {
                info!(
                    "{} response code: {response_code}",
                    self.request_summary(session, ctx)
                );
            }
        }
    }
}
