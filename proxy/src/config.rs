use std::{env, path::PathBuf, time::Duration};

#[derive(Debug, Clone)]
pub struct Config {
    pub network: String,
    pub proxy_addr: String,
    pub proxy_namespace: String,
    pub proxy_tiers_path: PathBuf,
    pub proxy_tiers_poll_interval: Duration,
    pub prometheus_addr: String,
    pub ssl_crt_path: String,
    pub ssl_key_path: String,
    pub kupo_port: u16,
    pub kupo_dns: String,

    // Health endpoint
    pub health_endpoint: String,
    pub health_poll_interval: std::time::Duration,
    pub private_endpoint: String,
}
impl Config {
    pub fn new() -> Self {
        let private_endpoint = env::var("KUPO_PRIVATE_ENDPOINT_REGEX")
            .unwrap_or(r"^PUT/patterns(?:/.*)?$".to_string());

        Self {
            network: env::var("NETWORK").expect("NETWORK must be set"),
            proxy_addr: env::var("PROXY_ADDR").expect("PROXY_ADDR must be set"),
            proxy_namespace: env::var("PROXY_NAMESPACE").expect("PROXY_NAMESPACE must be set"),
            proxy_tiers_path: env::var("PROXY_TIERS_PATH")
                .map(|v| v.into())
                .expect("PROXY_TIERS_PATH must be set"),
            proxy_tiers_poll_interval: env::var("PROXY_TIERS_POLL_INTERVAL")
                .map(|v| {
                    Duration::from_secs(
                        v.parse::<u64>()
                            .expect("PROXY_TIERS_POLL_INTERVAL must be a number in seconds. eg: 2"),
                    )
                })
                .unwrap_or(Duration::from_secs(2)),
            prometheus_addr: env::var("PROMETHEUS_ADDR").expect("PROMETHEUS_ADDR must be set"),
            ssl_crt_path: env::var("SSL_CRT_PATH").expect("SSL_CRT_PATH must be set"),
            ssl_key_path: env::var("SSL_KEY_PATH").expect("SSL_KEY_PATH must be set"),
            kupo_port: env::var("KUPO_PORT")
                .expect("KUPO_PORT must be set")
                .parse()
                .expect("KUPO_PORT must a number"),
            kupo_dns: env::var("KUPO_DNS").expect("KUPO_DNS must be set"),
            health_endpoint: "/dmtr_health".to_string(),
            health_poll_interval: env::var("HEALTH_POLL_INTERVAL")
                .map(|v| {
                    Duration::from_secs(
                        v.parse::<u64>()
                            .expect("HEALTH_POLL_INTERVAL must be a number in seconds. eg: 2"),
                    )
                })
                .unwrap_or(Duration::from_secs(10)),
            private_endpoint,
        }
    }

    pub fn instance(&self, pruned: bool) -> String {
        match pruned {
            true => format!(
                "kupo-{}-pruned.{}:{}",
                self.network, self.kupo_dns, self.kupo_port
            ),
            false => format!("kupo-{}.{}:{}", self.network, self.kupo_dns, self.kupo_port),
        }
    }
}
impl Default for Config {
    fn default() -> Self {
        Self::new()
    }
}
