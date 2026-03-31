use crate::http::AppState;
use crate::ipc;
use crate::ipc::command_id_from_context;
use crate::ipc::updater::UpdaterConnection;
use helios_updater::client::{CommandId, UpdaterSession};
use helios_updater::ipc::{PreflightReport, UpdateState, UpdaterCommand, UpdaterStorageReport};
use lib_ipc::protocol::ControlEvent;
use std::future::Future;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::timeout;

pub async fn ensure_updater(state: &AppState) -> Result<Arc<UpdaterConnection>, String> {
    let mut guard = state.updater.lock().await;
    if let Some(conn) = guard.as_ref() {
        return Ok(conn.clone());
    }
    match crate::ipc::updater::connect_updater().await {
        Ok(conn) => {
            let conn = Arc::new(conn);
            *guard = Some(conn.clone());
            Ok(conn)
        }
        Err(err) => Err(err.to_string()),
    }
}

pub async fn invalidate_updater(state: &AppState, conn: &Arc<UpdaterConnection>) {
    let mut guard = state.updater.lock().await;
    if let Some(current) = guard.as_ref()
        && Arc::ptr_eq(current, conn)
    {
        *guard = None;
    }
}

pub async fn send_query_state(conn: &UpdaterConnection, session: &mut UpdaterSession, label: &str) -> Result<(), String> {
    let command = UpdaterCommand::QueryState { command_id: command_id_from_context(label) };
    conn.client.journal().append(&command).map_err(|err| err.to_string())?;
    session.send_command(conn.client.journal(), &command).await.map_err(|err| err.to_string())?;
    Ok(())
}

pub async fn send_updater_command(state: &AppState, command: UpdaterCommand, wait_for_ack: bool) -> Result<(), String> {
    let command_id = command_id(&command);
    with_updater(state, |conn| async move {
        let journal = conn.client.journal();
        let mut session = conn.checkout_session().await.map_err(|err| err.to_string())?;

        journal.append(&command).map_err(|err| err.to_string())?;
        session.send_command(journal, &command).await.map_err(|err| err.to_string())?;

        if !wait_for_ack {
            conn.recycle_session(session).await;
            return Ok(());
        }

        let deadline = Duration::from_secs(10);
        let mut attempts = 0;
        let result = loop {
            attempts += 1;
            let event = match timeout(deadline, session.next_event()).await {
                Ok(Ok(Some(event))) => event,
                Ok(Ok(None)) => break Err("updater closed connection".to_string()),
                Ok(Err(err)) => break Err(err.to_string()),
                Err(_) => break Err("timed out waiting for updater response".to_string()),
            };

            match event {
                helios_updater::ipc::UpdaterEvent::Control(ControlEvent::Ack(ack)) if ack.command_id == command_id => break Ok(()),
                helios_updater::ipc::UpdaterEvent::Control(ControlEvent::Nack(nack)) if nack.command_id == command_id => break Err(nack.reason),
                _ => {
                    if attempts > 32 {
                        break Err("updater did not acknowledge command".to_string());
                    }
                }
            }
        };

        if result.is_ok() {
            conn.recycle_session(session).await;
        }
        result
    })
    .await
}

pub async fn fetch_updater_state(state: &AppState) -> Result<(Option<UpdateState>, u64), String> {
    with_updater(state, |conn| async move {
        let journal = conn.client.journal();
        let command = UpdaterCommand::QueryState { command_id: command_id_from_context("ota_state") };
        let cmd_id = command_id(&command);
        let mut session = conn.checkout_session().await.map_err(|err| err.to_string())?;
        journal.append(&command).map_err(|err| err.to_string())?;
        session.send_command(journal, &command).await.map_err(|err| err.to_string())?;

        let deadline = Duration::from_secs(5);
        let result = loop {
            let event = match timeout(deadline, session.next_event()).await {
                Ok(Ok(Some(event))) => event,
                Ok(Ok(None)) => break Err("updater closed connection".to_string()),
                Ok(Err(err)) => break Err(err.to_string()),
                Err(_) => break Err("timed out waiting for updater state".to_string()),
            };

            match event {
                helios_updater::ipc::UpdaterEvent::StateSnapshot { active_update, cache_usage_bytes } => break Ok((active_update, cache_usage_bytes)),
                helios_updater::ipc::UpdaterEvent::Control(ControlEvent::Nack(nack)) if nack.command_id == cmd_id => break Err(nack.reason),
                _ => continue,
            }
        };

        if result.is_ok() {
            conn.recycle_session(session).await;
        }
        result
    })
    .await
}

pub async fn fetch_updater_storage(state: &AppState) -> Result<UpdaterStorageReport, String> {
    with_updater(state, |conn| async move {
        let journal = conn.client.journal();
        let command = UpdaterCommand::QueryStorage { command_id: command_id_from_context("ota_storage") };
        let cmd_id = command_id(&command);
        let mut session = conn.checkout_session().await.map_err(|err| err.to_string())?;
        journal.append(&command).map_err(|err| err.to_string())?;
        session.send_command(journal, &command).await.map_err(|err| err.to_string())?;

        let deadline = Duration::from_secs(5);
        let result = loop {
            let event = match timeout(deadline, session.next_event()).await {
                Ok(Ok(Some(event))) => event,
                Ok(Ok(None)) => break Err("updater closed connection".to_string()),
                Ok(Err(err)) => break Err(err.to_string()),
                Err(_) => break Err("timed out waiting for updater storage report".to_string()),
            };

            match event {
                helios_updater::ipc::UpdaterEvent::StorageReport { report } => break Ok(report),
                helios_updater::ipc::UpdaterEvent::Control(ControlEvent::Nack(nack)) if nack.command_id == cmd_id => break Err(nack.reason),
                _ => continue,
            }
        };

        if result.is_ok() {
            conn.recycle_session(session).await;
        }
        result
    })
    .await
}

pub async fn fetch_updater_preflight(state: &AppState, update_id: uuid::Uuid) -> Result<PreflightReport, String> {
    with_updater(state, |conn| async move {
        let journal = conn.client.journal();
        let command = UpdaterCommand::PreflightRelease { command_id: command_id_from_context("ota_preflight"), update_id };
        let cmd_id = command_id(&command);
        let mut session = conn.checkout_session().await.map_err(|err| err.to_string())?;
        journal.append(&command).map_err(|err| err.to_string())?;
        session.send_command(journal, &command).await.map_err(|err| err.to_string())?;

        let deadline = Duration::from_secs(60);
        let result = loop {
            let event = match timeout(deadline, session.next_event()).await {
                Ok(Ok(Some(event))) => event,
                Ok(Ok(None)) => break Err("updater closed connection".to_string()),
                Ok(Err(err)) => break Err(err.to_string()),
                Err(_) => break Err("timed out waiting for updater preflight".to_string()),
            };

            match event {
                helios_updater::ipc::UpdaterEvent::PreflightReport { report } if report.update_id == update_id => break Ok(report),
                helios_updater::ipc::UpdaterEvent::Control(ControlEvent::Nack(nack)) if nack.command_id == cmd_id => break Err(nack.reason),
                _ => continue,
            }
        };

        if result.is_ok() {
            conn.recycle_session(session).await;
        }
        result
    })
    .await
}

fn command_id(command: &UpdaterCommand) -> CommandId {
    match command {
        UpdaterCommand::StageRelease { command_id, .. }
        | UpdaterCommand::Cancel { command_id, .. }
        | UpdaterCommand::ApplyRelease { command_id, .. }
        | UpdaterCommand::Rollback { command_id, .. }
        | UpdaterCommand::QueryState { command_id }
        | UpdaterCommand::QueryStorage { command_id }
        | UpdaterCommand::PreflightRelease { command_id, .. } => *command_id,
    }
}

async fn with_updater<F, Fut, T>(state: &AppState, f: F) -> Result<T, String>
where
    F: FnOnce(Arc<UpdaterConnection>) -> Fut,
    Fut: Future<Output = Result<T, String>>,
{
    let conn = {
        let mut guard = state.updater.lock().await;
        if let Some(conn) = guard.as_ref() {
            conn.clone()
        } else {
            let conn = Arc::new(ipc::updater::connect_updater().await.map_err(|err| err.to_string())?);
            *guard = Some(conn.clone());
            conn
        }
    };

    let result = f(conn.clone()).await;

    if result.is_err() {
        let mut guard = state.updater.lock().await;
        if let Some(current) = guard.as_ref()
            && Arc::ptr_eq(current, &conn)
        {
            *guard = None;
        }
    }

    result
}
