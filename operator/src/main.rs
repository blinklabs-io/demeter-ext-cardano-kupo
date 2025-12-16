use dotenv::dotenv;
use std::{io, sync::Arc};
use tracing::Level;

use operator::{controller, metrics as metrics_collector, State};

#[tokio::main]
async fn main() -> io::Result<()> {
    dotenv().ok();

    tracing_subscriber::fmt().with_max_level(Level::INFO).init();

    let state = Arc::new(State::default());

    metrics_collector::run_metrics_collector(state.clone());
    metrics_collector::run_metrics_server(state.clone());

    controller::run(state.clone()).await;

    Ok(())
}
