use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("Kube Error: {0}")]
    KubeError(#[source] kube::Error),

    #[error("Finalizer Error: {0}")]
    FinalizerError(#[source] Box<kube::runtime::finalizer::Error<Error>>),

    #[error("Deserialize Error: {0}")]
    DeserializeError(#[source] serde_json::Error),
}

impl Error {
    pub fn metric_label(&self) -> String {
        format!("{self:?}").to_lowercase()
    }
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

impl From<serde_json::Error> for Error {
    fn from(value: serde_json::Error) -> Self {
        Error::DeserializeError(value)
    }
}

impl From<kube::Error> for Error {
    fn from(value: kube::Error) -> Self {
        Error::KubeError(value)
    }
}

pub struct Config {}
impl Config {
    pub fn new() -> Self {
        Self {}
    }
}
impl Default for Config {
    fn default() -> Self {
        Self::new()
    }
}

pub mod controller;
pub use crate::controller::*;

mod metrics;
pub use metrics::Metrics;

mod helpers;
pub use helpers::*;

mod handlers;
pub use handlers::*;

mod config;
pub use config::*;
