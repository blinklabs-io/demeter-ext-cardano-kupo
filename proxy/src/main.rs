use dotenv::dotenv;
use operator::{kube::ResourceExt, KupoPort};
use pingora::{
    server::{configuration::Opt, Server},
    services::background::background_service,
};
use pingora_limits::rate::Rate;
use prometheus::{histogram_opts, opts, register_histogram_vec, register_int_counter_vec};
use regex::Regex;
use serde::{Deserialize, Deserializer};
use std::{collections::HashMap, fmt::Display, sync::Arc, time::Duration};
use tokio::sync::RwLock;
use tracing::Level;

mod auth;
mod config;
mod health;
mod proxy;
mod tiers;
mod utils;

use auth::AuthBackgroundService;
use config::Config;
use health::HealthBackgroundService;
use proxy::KupoProxy;
use tiers::TierBackgroundService;

use crate::utils::handle_legacy_networks;

fn main() {
    dotenv().ok();

    tracing_subscriber::fmt().with_max_level(Level::INFO).init();

    let config: Arc<Config> = Arc::default();
    let state: Arc<State> = Arc::default();

    let opt = Opt::default();
    let mut server = Server::new(Some(opt)).unwrap();
    server.bootstrap();

    let auth_background_service = background_service(
        "K8S Auth Service",
        AuthBackgroundService::new(state.clone()),
    );
    server.add_service(auth_background_service);

    let tier_background_service = background_service(
        "K8S Tier Service",
        TierBackgroundService::new(state.clone(), config.clone()),
    );
    server.add_service(tier_background_service);

    let mut kupo_http_proxy = pingora::proxy::http_proxy_service(
        &server.configuration,
        KupoProxy::new(state.clone(), config.clone()),
    );
    kupo_http_proxy
        .add_tls(
            &config.proxy_addr,
            &config.ssl_crt_path,
            &config.ssl_key_path,
        )
        .unwrap();
    server.add_service(kupo_http_proxy);

    let mut prometheus_service = pingora::services::listening::Service::prometheus_http_service();
    prometheus_service.add_tcp(&config.prometheus_addr);
    server.add_service(prometheus_service);

    let health_background_service = background_service(
        "K8S Auth Service",
        HealthBackgroundService::new(state.clone(), config.clone()),
    );
    server.add_service(health_background_service);

    server.run_forever();
}

#[derive(Default)]
pub struct State {
    consumers: RwLock<HashMap<String, Consumer>>,
    tiers: RwLock<HashMap<String, Tier>>,
    limiter: RwLock<HashMap<String, Vec<(TierRate, Rate)>>>,
    metrics: Metrics,
    upstream_health: RwLock<bool>,
}
impl State {
    pub async fn get_consumer(&self, key: &str) -> Option<Consumer> {
        self.consumers.read().await.get(key).cloned()
    }
}

#[derive(Debug, Clone, Default)]
pub struct Consumer {
    namespace: String,
    port_name: String,
    tier: String,
    key: String,
    network: String,
    pruned: bool,
}
impl Display for Consumer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}", self.namespace, self.port_name)
    }
}
impl From<&KupoPort> for Consumer {
    fn from(value: &KupoPort) -> Self {
        let network = handle_legacy_networks(&value.spec.network);
        let tier = value.spec.throughput_tier.to_string();
        let key = value.status.as_ref().unwrap().auth_token.clone();
        let namespace = value.metadata.namespace.as_ref().unwrap().clone();
        let port_name = value.name_any();
        let pruned = value.spec.prune_utxo;

        Self {
            namespace,
            port_name,
            tier,
            key,
            network,
            pruned,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct Tier {
    name: String,
    rates: Vec<TierRate>,
}
#[derive(Debug, Clone, Deserialize)]
pub struct TierRate {
    limit: isize,
    #[serde(deserialize_with = "deserialize_duration")]
    interval: Duration,
}
pub fn deserialize_duration<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Duration, D::Error> {
    let value: String = Deserialize::deserialize(deserializer)?;
    let regex = Regex::new(r"([\d]+)([\w])").unwrap();
    let captures = regex.captures(&value);
    if captures.is_none() {
        return Err(<D::Error as serde::de::Error>::custom(
            "Invalid tier interval format",
        ));
    }

    let captures = captures.unwrap();
    let number = captures.get(1).unwrap().as_str().parse::<u64>().unwrap();
    let symbol = captures.get(2).unwrap().as_str();

    match symbol {
        "s" => Ok(Duration::from_secs(number)),
        "m" => Ok(Duration::from_secs(number * 60)),
        "h" => Ok(Duration::from_secs(number * 60 * 60)),
        "d" => Ok(Duration::from_secs(number * 60 * 60 * 24)),
        _ => Err(<D::Error as serde::de::Error>::custom(
            "Invalid symbol tier interval",
        )),
    }
}

#[derive(Debug, Clone)]
pub struct Metrics {
    http_total_request: prometheus::IntCounterVec,
    http_request_duration_seconds: prometheus::HistogramVec,
}
impl Metrics {
    pub fn new() -> Self {
        let http_total_request = register_int_counter_vec!(
            opts!("kupo_proxy_http_total_request", "Total http request",),
            &["consumer", "namespace", "instance", "status_code", "tier"]
        )
        .unwrap();

        let http_request_duration_seconds = register_histogram_vec!(
            histogram_opts!(
                "kupo_proxy_http_request_duration_seconds",
                "HTTP request duration in seconds",
                vec![
                    0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0, 20.0, 40.0,
                    60.0, 90.0, 120.0
                ]
            ),
            &["status_code", "network"]
        )
        .unwrap();

        Self {
            http_total_request,
            http_request_duration_seconds,
        }
    }

    pub fn inc_http_total_request(
        &self,
        consumer: &Consumer,
        namespace: &str,
        instance: &str,
        status: &u16,
    ) {
        self.http_total_request
            .with_label_values(&[
                &consumer.to_string(),
                namespace,
                instance,
                &status.to_string(),
                &consumer.tier,
            ])
            .inc()
    }

    /// Observe HTTP request duration in seconds.
    pub fn observe_http_request_duration(
        &self,
        consumer: &Consumer,
        status: &u16,
        duration: std::time::Duration,
    ) {
        self.http_request_duration_seconds
            .with_label_values(&[&status.to_string(), &consumer.network])
            .observe(duration.as_secs_f64());
    }
}
impl Default for Metrics {
    fn default() -> Self {
        Self::new()
    }
}
