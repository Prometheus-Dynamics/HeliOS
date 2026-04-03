mod session;
mod shutdown;

use std::io::ErrorKind;
use std::sync::Arc;
use std::time::Instant;

use futures::FutureExt;
use lib_ipc::journal::JournalWriter;
use lib_ipc::wire::ServiceKind;
use tokio::fs;
use tokio::net::UnixListener;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};

use crate::config::UpdaterConfig;
use crate::error::{Error, Result};
use crate::service::UpdaterService;

use self::session::handle_connection;
use self::shutdown::wait_for_shutdown;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RuntimeState {
    Idle,
    Starting,
    Running,
    Stopping,
}

pub struct UpdaterRuntime {
    config: Arc<UpdaterConfig>,
    shutdown: CancellationToken,
    state: RuntimeState,
    service: Option<Arc<UpdaterService>>,
    started_at: Instant,
}

impl Default for UpdaterRuntime {
    fn default() -> Self {
        Self::new()
    }
}

impl UpdaterRuntime {
    pub fn new() -> Self {
        Self::from_config(UpdaterConfig::default())
    }

    pub fn from_config(config: UpdaterConfig) -> Self {
        Self { config: Arc::new(config), shutdown: CancellationToken::new(), state: RuntimeState::Idle, service: None, started_at: Instant::now() }
    }

    pub fn config(&self) -> &UpdaterConfig {
        &self.config
    }

    pub fn shutdown_token(&self) -> CancellationToken {
        self.shutdown.clone()
    }

    pub fn request_shutdown(&self) {
        self.shutdown.cancel();
    }

    pub async fn start(&mut self) -> Result<()> {
        if matches!(self.state, RuntimeState::Starting | RuntimeState::Running) {
            return Err(Error::InvalidState("updater runtime already running".into()));
        }

        info!("updater runtime starting");
        self.state = RuntimeState::Starting;
        self.started_at = Instant::now();

        let journal = Arc::new(JournalWriter::open(self.config.journal_path(), ServiceKind::Updater)?);
        let service = Arc::new(UpdaterService::new(Arc::clone(&self.config))?);
        if let Err(err) = service.run_post_boot_cleanup().await {
            warn!(%err, "post-boot cleanup task failed");
        }
        self.service = Some(service.clone());

        if let Some(parent) = self.config.socket_path().parent() {
            fs::create_dir_all(parent).await?;
        }
        if fs::metadata(self.config.socket_path()).await.is_ok() {
            fs::remove_file(self.config.socket_path()).await?;
        }

        let listener = UnixListener::bind(self.config.socket_path())?;
        self.state = RuntimeState::Running;
        info!(path = %self.config.socket_path().display(), "updater runtime listening");

        let mut tasks = JoinSet::new();
        let shutdown_future = wait_for_shutdown(self.shutdown.clone()).fuse();
        tokio::pin!(shutdown_future);

        let shutdown_reason: &'static str = loop {
            tokio::select! {
                reason = &mut shutdown_future => break reason?,
                accept = listener.accept() => {
                    match accept {
                        Ok((stream, _addr)) => {
                            let config = Arc::clone(&self.config);
                            let journal = Arc::clone(&journal);
                            let shutdown = self.shutdown.child_token();
                            let service = Arc::clone(&service);
                            let started_at = self.started_at;
                            tasks.spawn(handle_connection(stream, config, journal, shutdown, service, started_at));
                        }
                        Err(err) => {
                            error!(%err, "failed to accept updater client");
                        }
                    }
                }
            }
        };

        self.state = RuntimeState::Stopping;
        info!(reason = shutdown_reason, "updater runtime stopping");
        self.shutdown.cancel();

        while let Some(result) = tasks.join_next().await {
            match result {
                Ok(Ok(())) => {}
                Ok(Err(err)) => warn!(%err, "updater client session ended with error"),
                Err(join_err) => warn!(%join_err, "updater client task panicked"),
            }
        }

        drop(listener);
        if let Err(err) = fs::remove_file(self.config.socket_path()).await
            && err.kind() != ErrorKind::NotFound
        {
            warn!(%err, "failed to remove updater socket");
        }

        self.state = RuntimeState::Idle;
        self.shutdown = CancellationToken::new();
        self.service = None;
        info!("updater runtime stopped");
        Ok(())
    }
}
