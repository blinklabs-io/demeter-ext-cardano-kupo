use chrono::Utc;
use http_body_util::{combinators::BoxBody, BodyExt, Full};
use hyper::{body::Bytes, server::conn::http1, service::service_fn, Response};
use hyper_util::rt::TokioIo;
use kube::{Resource, ResourceExt};
use prometheus::{opts, Encoder, IntCounterVec, Registry, TextEncoder};
use regex::Regex;
use serde::{Deserialize, Deserializer};
use std::{net::SocketAddr, str::FromStr, sync::Arc};
use tokio::net::TcpListener;
use tracing::{error, info, instrument, warn};

use crate::{get_config, Config, Error, KupoPort, State};

#[derive(Clone)]
pub struct Metrics {
    pub usage: IntCounterVec,
    pub reconcile_failures: IntCounterVec,
    pub metrics_failures: IntCounterVec,
}

impl Default for Metrics {
    fn default() -> Self {
        let usage = IntCounterVec::new(
            opts!("usage", "Feature usage",),
            &["feature", "project", "resource_name", "tier"],
        )
        .unwrap();

        let reconcile_failures = IntCounterVec::new(
            opts!(
                "kupo_operator_crd_reconciliation_errors_total",
                "reconciliation errors",
            ),
            &["instance", "error"],
        )
        .unwrap();

        let metrics_failures = IntCounterVec::new(
            opts!(
                "kupo_operator_metrics_errors_total",
                "errors to calculation metrics",
            ),
            &["error"],
        )
        .unwrap();

        Metrics {
            usage,
            reconcile_failures,
            metrics_failures,
        }
    }
}

impl Metrics {
    pub fn register(self, registry: &Registry) -> Result<Self, prometheus::Error> {
        registry.register(Box::new(self.reconcile_failures.clone()))?;
        registry.register(Box::new(self.metrics_failures.clone()))?;
        registry.register(Box::new(self.usage.clone()))?;

        Ok(self)
    }

    pub fn reconcile_failure(&self, crd: &KupoPort, e: &Error) {
        self.reconcile_failures
            .with_label_values(&[crd.name_any().as_ref(), e.metric_label().as_ref()])
            .inc()
    }

    pub fn metrics_failure(&self, e: &Error) {
        self.metrics_failures
            .with_label_values(&[e.metric_label().as_ref()])
            .inc()
    }

    pub fn count_usage(&self, project: &str, resource_name: &str, tier: &str, value: f64) {
        let feature = &KupoPort::kind(&());
        let value: u64 = value.ceil() as u64;

        self.usage
            .with_label_values(&[feature, project, resource_name, tier])
            .inc_by(value);
    }
}

async fn api_get_metrics(
    state: Arc<State>,
) -> Result<Response<BoxBody<Bytes, hyper::Error>>, hyper::Error> {
    let metrics = state.metrics_collected();

    let encoder = TextEncoder::new();
    let mut buffer = vec![];
    encoder.encode(&metrics, &mut buffer).unwrap();

    let res = Response::builder()
        .body(
            Full::new(buffer.into())
                .map_err(|never| match never {})
                .boxed(),
        )
        .unwrap();
    Ok(res)
}

pub fn run_metrics_server(state: Arc<State>) {
    tokio::spawn(async move {
        let addr = std::env::var("ADDR").unwrap_or("0.0.0.0:8080".into());
        let addr_result = SocketAddr::from_str(&addr);
        if let Err(err) = addr_result {
            error!(error = err.to_string(), "invalid prometheus addr");
            std::process::exit(1);
        }
        let addr = addr_result.unwrap();

        let listener_result = TcpListener::bind(addr).await;
        if let Err(err) = listener_result {
            error!(
                error = err.to_string(),
                "fail to bind tcp prometheus server listener"
            );
            std::process::exit(1);
        }
        let listener = listener_result.unwrap();

        info!(addr = addr.to_string(), "metrics listening");

        loop {
            let state = state.clone();

            let accept_result = listener.accept().await;
            if let Err(err) = accept_result {
                error!(error = err.to_string(), "accept client prometheus server");
                continue;
            }
            let (stream, _) = accept_result.unwrap();

            let io = TokioIo::new(stream);

            tokio::task::spawn(async move {
                let service = service_fn(move |_| api_get_metrics(state.clone()));

                if let Err(err) = http1::Builder::new().serve_connection(io, service).await {
                    error!(error = err.to_string(), "failed metrics server connection");
                }
            });
        }
    });
}

async fn collect_prometheus_metrics(
    config: &Config,
    query: String,
) -> Result<PrometheusResponse, Error> {
    let client = reqwest::Client::builder().build().unwrap();

    let response = client
        .get(format!("{}/query?query={query}", config.prometheus_url))
        .send()
        .await?;

    let status = response.status();
    if status.is_client_error() || status.is_server_error() {
        error!(status = status.to_string(), "request status code fail");
        return Err(Error::HttpError(format!(
            "Prometheus request error. Status: {} Query: {}",
            status, query
        )));
    }

    Ok(response.json().await.unwrap())
}

#[instrument("metrics collector run", skip_all)]
pub fn run_metrics_collector(state: Arc<State>) {
    tokio::spawn(async move {
        info!("collecting metrics running");

        let config = get_config();
        let project_regex = Regex::new(r"prj-(.+)\.(.+)$").unwrap();
        let network_regex = Regex::new(r"kupo-([\w-]+)-.+").unwrap();
        let mut last_execution = Utc::now();

        loop {
            tokio::time::sleep(config.metrics_delay).await;

            let end = Utc::now();
            let start = (end - last_execution).num_seconds();

            last_execution = end;

            let query = format!(
                "sum by (consumer, exported_instance, tier) (increase(kupo_proxy_http_total_request{{status_code!~\"401|429|503\"}}[{start}s] @ {}))",
                end.timestamp_millis() / 1000
            );

            let response = collect_prometheus_metrics(config, query).await;
            if let Err(err) = response {
                error!(error = err.to_string(), "error to make prometheus request");
                state.metrics.metrics_failure(&err);
                continue;
            }
            let response = response.unwrap();

            for result in response.data.result {
                if result.value == 0.0
                    || result.metric.consumer.is_none()
                    || result.metric.exported_instance.is_none()
                    || result.metric.tier.is_none()
                {
                    continue;
                }

                let consumer = result.metric.consumer.unwrap();
                let project_captures = project_regex.captures(&consumer);
                if project_captures.is_none() {
                    warn!(consumer, "invalid project to the regex");
                    continue;
                }
                let project_captures = project_captures.unwrap();
                let project = project_captures.get(1).unwrap().as_str();
                let resource_name = project_captures.get(2).unwrap().as_str();

                let instance = result.metric.exported_instance.unwrap();
                let network_captures = network_regex.captures(&instance);
                if network_captures.is_none() {
                    warn!(instance, "invalid network to the regex");
                    continue;
                }
                let tier = result.metric.tier.unwrap();

                state
                    .metrics
                    .count_usage(project, resource_name, &tier, result.value);
            }
        }
    });
}

#[derive(Debug, Deserialize)]
struct PrometheusDataResultMetric {
    consumer: Option<String>,
    exported_instance: Option<String>,
    tier: Option<String>,
}

#[derive(Debug, Deserialize)]
struct PrometheusDataResult {
    metric: PrometheusDataResultMetric,
    #[serde(deserialize_with = "deserialize_value")]
    value: f64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PrometheusData {
    result: Vec<PrometheusDataResult>,
}

#[derive(Debug, Deserialize)]
struct PrometheusResponse {
    data: PrometheusData,
}

fn deserialize_value<'de, D>(deserializer: D) -> Result<f64, D::Error>
where
    D: Deserializer<'de>,
{
    let value: Vec<serde_json::Value> = Deserialize::deserialize(deserializer)?;
    Ok(value.into_iter().as_slice()[1]
        .as_str()
        .unwrap()
        .parse::<f64>()
        .unwrap())
}
