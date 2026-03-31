use std::{sync::Arc, time::Duration};

use helios_updater::client::UpdaterSession;
use helios_updater::ipc::UpdaterCommand;
use lib_ipc::types::CommandId;
use tokio::time::sleep;
use tracing::{info, warn};

use crate::ipc::IpcHandles;
use crate::ipc::updater::UpdaterConnection;

use super::config::{EngineCrashLightingConfig, UpdateLightingConfig};
use super::lighting::{apply_update_lighting, flash_engine_crash_led};
use super::state::{UpdateLedMode, UpdateLedState, update_mode_from_event};

pub(crate) fn spawn_update_led_task(handles: Arc<IpcHandles>) -> Arc<UpdateLedState> {
    let cfg = UpdateLightingConfig::from_env();
    let update_state = Arc::new(UpdateLedState::default());
    if !cfg.enabled {
        return update_state;
    }
    tokio::spawn(run_update_led_loop(handles, cfg, update_state.clone()));
    update_state
}

pub(crate) fn spawn_engine_crash_led_task(handles: Arc<IpcHandles>, update_state: Arc<UpdateLedState>) {
    let cfg = EngineCrashLightingConfig::from_env();
    if !cfg.enabled {
        return;
    }
    let update_cfg = UpdateLightingConfig::from_env();
    tokio::spawn(run_engine_crash_led_loop(handles, cfg, update_state, update_cfg));
}

async fn run_update_led_loop(handles: Arc<IpcHandles>, cfg: UpdateLightingConfig, update_state: Arc<UpdateLedState>) {
    let mut last_mode: Option<UpdateLedMode> = None;
    loop {
        let updater = { handles.updater.lock().await.clone() };
        let Some(updater) = updater else {
            sleep(cfg.retry_delay).await;
            continue;
        };

        let mut session = match updater.checkout_session().await {
            Ok(session) => session,
            Err(err) => {
                warn!(%err, "update lighting: failed to checkout updater session");
                sleep(cfg.retry_delay).await;
                continue;
            }
        };

        if let Err(err) = send_query_state(&updater, &mut session).await {
            warn!(%err, "update lighting: failed to query updater state");
        }

        loop {
            match session.next_event().await {
                Ok(Some(event)) => {
                    if let Some(mode) = update_mode_from_event(&event, cfg.reboot_grace)
                        && last_mode != Some(mode)
                    {
                        update_state.set_mode(mode);
                        if apply_update_lighting(&handles, &cfg, mode).await {
                            last_mode = Some(mode);
                        }
                    }
                }
                Ok(None) => {
                    info!("update lighting: updater session closed");
                    break;
                }
                Err(err) => {
                    warn!(%err, "update lighting: updater session error");
                    break;
                }
            }
        }

        sleep(cfg.retry_delay).await;
    }
}

async fn run_engine_crash_led_loop(handles: Arc<IpcHandles>, cfg: EngineCrashLightingConfig, update_state: Arc<UpdateLedState>, update_cfg: UpdateLightingConfig) {
    let mut last_seen = 0u64;
    loop {
        sleep(Duration::from_millis(cfg.poll_ms)).await;
        let Some(ts) = handles.engine.last_disconnect_ms() else {
            continue;
        };
        if ts <= last_seen {
            continue;
        }
        last_seen = ts;
        flash_engine_crash_led(&handles, &cfg).await;
        let mode = update_state.mode();
        if mode != UpdateLedMode::Idle {
            let _ = apply_update_lighting(&handles, &update_cfg, mode).await;
        }
    }
}

async fn send_query_state(updater: &UpdaterConnection, session: &mut UpdaterSession) -> Result<(), String> {
    let command = UpdaterCommand::QueryState { command_id: CommandId::new() };
    session.send_command(updater.client.journal(), &command).await.map_err(|err| err.to_string())?;
    Ok(())
}
