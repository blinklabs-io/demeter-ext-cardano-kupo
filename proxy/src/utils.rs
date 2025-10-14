use lazy_static::lazy_static;
use std::collections::HashMap;

lazy_static! {
    static ref LEGACY_NETWORKS: HashMap<&'static str, String> = {
        let mut m = HashMap::new();
        m.insert("mainnet", "cardano-mainnet".into());
        m.insert("preprod", "cardano-preprod".into());
        m.insert("preview", "cardano-preview".into());
        m
    };
}

pub fn handle_legacy_networks(network: &str) -> String {
    let default = network.to_string();
    LEGACY_NETWORKS.get(network).unwrap_or(&default).to_string()
}
