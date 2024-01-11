use futures::StreamExt;
use kube::{
    api::ListParams,
    runtime::{controller::Action, watcher::Config as WatcherConfig, Controller},
    Api, Client, CustomResource, CustomResourceExt, ResourceExt,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::{sync::Arc, time::Duration};
use tracing::{error, info, instrument};

use crate::{
    auth::handle_auth,
    gateway::{handle_http_route, handle_http_route_key, handle_reference_grant},
    patch_resource_status, Error, Metrics, Network, Result, State,
};

pub static KUPO_PORT_FINALIZER: &str = "kupoports.demeter.run";

struct Context {
    pub client: Client,
    pub metrics: Metrics,
}
impl Context {
    pub fn new(client: Client, metrics: Metrics) -> Self {
        Self { client, metrics }
    }
}

#[derive(CustomResource, Deserialize, Serialize, Clone, Debug, JsonSchema)]
#[kube(
    kind = "KupoPort",
    group = "demeter.run",
    version = "v1alpha1",
    shortname = "kpts",
    namespaced
)]
#[kube(status = "KupoPortStatus")]
#[kube(printcolumn = r#"
        {"name": "Network", "jsonPath": ".spec.network", "type": "string"},
        {"name": "Pruned", "jsonPath": ".spec.pruneUtxo", "type": "boolean"},
        {"name": "Throughput Tier", "jsonPath":".spec.throughputTier", "type": "string"}, 
        {"name": "Endpoint URL", "jsonPath": ".status.endpointUrl", "type": "string"},
        {"name": "Endpoint Key URL", "jsonPath": ".status.endpoint_key_url", "type": "string"},
        {"name": "Auth Token", "jsonPath": ".status.authToken", "type": "string"}
    "#)]
#[serde(rename_all = "camelCase")]
pub struct KupoPortSpec {
    pub operator_version: String,
    pub network: Network,
    pub prune_utxo: bool,
    // throughput should be 0, 1, 2
    pub throughput_tier: String,
}

#[derive(Deserialize, Serialize, Clone, Default, Debug, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct KupoPortStatus {
    pub endpoint_url: String,
    pub endpoint_key_url: String,
    pub auth_token: String,
}

async fn reconcile(crd: Arc<KupoPort>, ctx: Arc<Context>) -> Result<Action> {
    handle_reference_grant(&ctx.client, &crd).await?;

    let key = handle_auth(&ctx.client, &crd).await?;
    let hostname = handle_http_route(&ctx.client, &crd).await?;
    let hostname_key = handle_http_route_key(&ctx.client, &crd, &key).await?;

    let status = KupoPortStatus {
        endpoint_url: format!("https://{hostname}"),
        endpoint_key_url: format!("https://{hostname_key}"),
        auth_token: key,
    };

    let namespace = crd.namespace().unwrap();
    let kupo_port = KupoPort::api_resource();

    patch_resource_status(
        ctx.client.clone(),
        &namespace,
        kupo_port,
        &crd.name_any(),
        serde_json::to_value(status)?,
    )
    .await?;

    info!(resource = crd.name_any(), "Reconcile completed");

    Ok(Action::await_change())
}

fn error_policy(crd: Arc<KupoPort>, err: &Error, ctx: Arc<Context>) -> Action {
    error!(error = err.to_string(), "reconcile failed");
    ctx.metrics.reconcile_failure(&crd, err);
    Action::requeue(Duration::from_secs(5))
}

#[instrument("controller run", skip_all)]
pub async fn run(state: Arc<State>) {
    info!("listening crds running");

    let client = Client::try_default()
        .await
        .expect("failed to create kube client");

    let crds = Api::<KupoPort>::all(client.clone());
    if let Err(e) = crds.list(&ListParams::default().limit(1)).await {
        error!("CRD is not queryable; {e:?}. Is the CRD installed?");
        std::process::exit(1);
    }

    let ctx = Context::new(client, state.metrics.clone());

    Controller::new(crds, WatcherConfig::default().any_semantic())
        .shutdown_on_signal()
        .run(reconcile, error_policy, Arc::new(ctx))
        .filter_map(|x| async move { std::result::Result::ok(x) })
        .for_each(|_| futures::future::ready(()))
        .await;
}
