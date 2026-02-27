use std::{fmt, net::SocketAddr};

use metrics_exporter_prometheus::{PrometheusBuilder, PrometheusHandle};
use once_cell::sync::OnceCell;

static PROMETHEUS_HANDLE: OnceCell<PrometheusHandle> = OnceCell::new();

#[derive(Debug)]
pub enum Error {
    RecorderInstall(metrics_exporter_prometheus::BuildError),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::RecorderInstall(err) => write!(f, "failed to install Prometheus recorder: {err}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<metrics_exporter_prometheus::BuildError> for Error {
    fn from(value: metrics_exporter_prometheus::BuildError) -> Self {
        Self::RecorderInstall(value)
    }
}

pub type Result<T> = core::result::Result<T, Error>;

pub fn init_prometheus(addr: SocketAddr) -> Result<&'static PrometheusHandle> {
    PROMETHEUS_HANDLE.get_or_try_init(|| PrometheusBuilder::new().with_http_listener(addr).install_recorder().map_err(Error::from))
}

pub fn prometheus_handle() -> Option<&'static PrometheusHandle> {
    PROMETHEUS_HANDLE.get()
}
