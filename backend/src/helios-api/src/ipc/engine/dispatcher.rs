use super::{EngineClient, EngineRequest, EngineSession, ExpectedEvent, JournalMode, disconnected_error, mark_disconnected};
use crate::ipc::engine::timeouts::{ENGINE_RECONNECT_INITIAL, ENGINE_RECONNECT_MAX, engine_command_send_timeout, scale_timeout, timeout_scale_for_streams};
use helios_engine::ipc::{EngineCommand, EngineEvent};
use lib_ipc::journal::JournalEntry;
use lib_ipc::types::CommandId;
use std::{
    collections::{HashMap, HashSet, VecDeque},
    future, io,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
    },
};
use tokio::sync::{broadcast, mpsc};
use tokio::time::{Duration, Instant, sleep, timeout};
use tracing::{debug, error, info, warn};

#[allow(clippy::too_many_arguments)]
pub(super) async fn run_engine_dispatcher(
    client: Arc<EngineClient>,
    mut rx: mpsc::Receiver<EngineRequest>,
    events: broadcast::Sender<EngineEvent>,
    connect_events: broadcast::Sender<()>,
    connected: Arc<AtomicBool>,
    last_disconnect_ms: Arc<AtomicU64>,
    timeout_scale_ppm: Arc<AtomicU64>,
    active_streams: Arc<AtomicUsize>,
) {
    let mut pending: HashMap<CommandId, EngineRequest> = HashMap::new();
    let mut journal_order: VecDeque<(CommandId, JournalEntry<EngineCommand>)> = VecDeque::new();
    let mut completed_journal_ids: HashSet<CommandId> = HashSet::new();
    let mut session: Option<EngineSession> = None;
    let mut backoff = ENGINE_RECONNECT_INITIAL;
    let mut rx_closed = false;

    loop {
        if session.is_none() {
            match timeout(Duration::from_secs(5), client.handshake()).await {
                Ok(Ok(sess)) => {
                    info!("connected to engine IPC");
                    session = Some(sess);
                    backoff = ENGINE_RECONNECT_INITIAL;
                    let _ = connect_events.send(());
                    connected.store(true, Ordering::Relaxed);
                }
                Ok(Err(err)) => {
                    error!(%err, "engine handshake failed, will retry");
                    flush_pending_disconnect(&mut pending, &mut journal_order, &mut completed_journal_ids, client.journal());
                    mark_disconnected(&connected, &last_disconnect_ms);
                    sleep(backoff).await;
                    backoff = (backoff * 2).min(ENGINE_RECONNECT_MAX);
                    continue;
                }
                Err(_) => {
                    error!("engine handshake timed out, will retry");
                    flush_pending_disconnect(&mut pending, &mut journal_order, &mut completed_journal_ids, client.journal());
                    mark_disconnected(&connected, &last_disconnect_ms);
                    sleep(backoff).await;
                    backoff = (backoff * 2).min(ENGINE_RECONNECT_MAX);
                    continue;
                }
            }
        }

        let next_deadline = pending.values().map(|req| req.deadline).min();
        let mut session_event = None;
        if let Some(sess) = session.as_mut() {
            session_event = Some(sess.next_event());
        }

        tokio::select! {
            biased;
            maybe_request = rx.recv() => {
                match maybe_request {
                    Some(request) => {
                        let mut disconnect = false;
                        if let Some(sess) = session.as_mut() {
                            let send_timeout = scale_timeout(
                                engine_command_send_timeout(),
                                timeout_scale_ppm.load(Ordering::Relaxed),
                            );
                            let send = match request.journal_mode {
                                JournalMode::Durable => {
                                    tokio::time::timeout(send_timeout, sess.send_command(client.journal(), &request.command))
                                        .await
                                        .map(|result| result.map(Some))
                                }
                                JournalMode::Ephemeral => {
                                    tokio::time::timeout(send_timeout, sess.send_ephemeral_command(&request.command))
                                        .await
                                        .map(|result| result.map(|()| None))
                                }
                            };
                            match send {
                                Ok(Ok(entry)) => {
                                    if let Some(entry) = entry {
                                        journal_order.push_back((request.command_id, entry));
                                    }
                                    pending.insert(request.command_id, request);
                                }
                                Ok(Err(err)) => {
                                    let _ = request.respond_to.send(Err(err));
                                }
                                Err(_) => {
                                    let command_id = request.command_id;
                                    let _ = request.respond_to.send(Err(
                                        lib_ipc::client::ClientTransportError::Io(io::Error::new(
                                            io::ErrorKind::TimedOut,
                                            "engine command send timed out",
                                        )),
                                    ));
                                    retire_journal_entry(
                                        command_id,
                                        &mut journal_order,
                                        &mut completed_journal_ids,
                                        client.journal(),
                                    );
                                    disconnect = true;
                                }
                            }
                        } else {
                            let command_id = request.command_id;
                            let _ = request.respond_to.send(Err(disconnected_error()));
                            retire_journal_entry(
                                command_id,
                                &mut journal_order,
                                &mut completed_journal_ids,
                                client.journal(),
                            );
                        }
                        if disconnect {
                            flush_pending_disconnect(
                                &mut pending,
                                &mut journal_order,
                                &mut completed_journal_ids,
                                client.journal(),
                            );
                            session = None;
                            mark_disconnected(&connected, &last_disconnect_ms);
                        }
                    }
                    None => {
                        rx_closed = true;
                        if pending.is_empty() {
                            break;
                        }
                    }
                }
            }
            event_result = async {
                if let Some(fut) = session_event {
                    fut.await
                } else {
                    Ok(None)
                }
            } => {
                let event = match event_result {
                    Ok(Some(ev)) => ev,
                    Ok(None) => {
                        flush_pending_disconnect(
                            &mut pending,
                            &mut journal_order,
                            &mut completed_journal_ids,
                            client.journal(),
                        );
                        session = None;
                        mark_disconnected(&connected, &last_disconnect_ms);
                        continue;
                    }
                    Err(err) => {
                        error!(%err, "engine session error; reconnecting");
                        flush_pending_disconnect(
                            &mut pending,
                            &mut journal_order,
                            &mut completed_journal_ids,
                            client.journal(),
                        );
                        session = None;
                        mark_disconnected(&connected, &last_disconnect_ms);
                        continue;
                    }
                };

                expire_timeouts(
                    &mut pending,
                    &mut journal_order,
                    &mut completed_journal_ids,
                    client.journal(),
                );

                update_scale_from_event(&event, &timeout_scale_ppm, &active_streams);

                let mut delivered = false;
                if let Some(command_id) = event.command_id()
                    && let Some(mut req) = pending.remove(&command_id)
                {
                    if matches!(req.expected, ExpectedEvent::Ack)
                        && matches!(event, EngineEvent::Ack { .. })
                        && !req.saw_transport_ack
                    {
                        req.saw_transport_ack = true;
                        pending.insert(command_id, req);
                        delivered = true;
                    } else if req.expected.matches(&event) {
                        let _ = req.respond_to.send(Ok(event.clone()));
                        retire_journal_entry(
                            command_id,
                            &mut journal_order,
                            &mut completed_journal_ids,
                            client.journal(),
                        );
                        delivered = true;
                    } else {
                        if !matches!(event, EngineEvent::Ack { .. }) {
                            warn!(?event, expected = ?req.expected, "engine returned unexpected event; keeping request pending");
                        }
                        pending.insert(command_id, req);
                        delivered = true;
                    }
                }

                if !delivered && matches!(event, EngineEvent::CalibrationSolved { .. }) {
                    let mut candidates: Vec<CommandId> = pending
                        .iter()
                        .filter_map(|(id, req)| matches!(req.expected, ExpectedEvent::CalibrationSolved).then_some(*id))
                        .collect();
                    if candidates.len() == 1 {
                        let id = candidates.pop().unwrap();
                        if let Some(req) = pending.remove(&id) {
                            if req.expected.matches(&event) {
                                let _ = req.respond_to.send(Ok(event.clone()));
                                retire_journal_entry(
                                    id,
                                    &mut journal_order,
                                    &mut completed_journal_ids,
                                    client.journal(),
                                );
                                delivered = true;
                            } else {
                                pending.insert(id, req);
                            }
                        }
                    } else if candidates.len() > 1 {
                        warn!(count = candidates.len(), "multiple pending calibration requests; ignoring unmatched CalibrationSolved event");
                    }
                }

                if !delivered {
                    if events.receiver_count() > 0 {
                        let _ = events.send(event.clone());
                    } else if event.command_id().is_some() {
                        debug!(?event, "dropping stale engine response");
                    } else if !matches!(event, EngineEvent::MetricsUpdate { .. }) {
                        warn!(?event, "received unsolicited engine event; no subscribers");
                    }
                }
            }
            _ = async {
                if let Some(deadline) = next_deadline {
                    tokio::time::sleep_until(deadline).await;
                } else {
                    future::pending::<()>().await;
                }
            } => {
                expire_timeouts(
                    &mut pending,
                    &mut journal_order,
                    &mut completed_journal_ids,
                    client.journal(),
                );
            }
        }

        if rx_closed && pending.is_empty() {
            break;
        }
    }
}

fn update_scale_from_event(event: &EngineEvent, timeout_scale_ppm: &AtomicU64, active_streams: &AtomicUsize) {
    match event {
        EngineEvent::StreamList { streams, .. } => {
            let count = streams.len();
            active_streams.store(count, Ordering::Relaxed);
            timeout_scale_ppm.store(timeout_scale_for_streams(count), Ordering::Relaxed);
        }
        EngineEvent::Started { .. } => {
            update_scale_from_delta(timeout_scale_ppm, active_streams, 1);
        }
        EngineEvent::Stopped { .. } => {
            update_scale_from_delta(timeout_scale_ppm, active_streams, -1);
        }
        _ => {}
    }
}

fn update_scale_from_delta(timeout_scale_ppm: &AtomicU64, active_streams: &AtomicUsize, delta: i64) {
    let mut current = active_streams.load(Ordering::Relaxed);
    let delta_abs = if delta < 0 { (-delta) as usize } else { delta as usize };
    loop {
        let next = if delta < 0 { current.saturating_sub(delta_abs) } else { current.saturating_add(delta_abs) };
        match active_streams.compare_exchange(current, next, Ordering::Relaxed, Ordering::Relaxed) {
            Ok(_) => {
                timeout_scale_ppm.store(timeout_scale_for_streams(next), Ordering::Relaxed);
                break;
            }
            Err(observed) => current = observed,
        }
    }
}

fn expire_timeouts(
    pending: &mut HashMap<CommandId, EngineRequest>,
    journal_order: &mut VecDeque<(CommandId, JournalEntry<EngineCommand>)>,
    completed_journal_ids: &mut HashSet<CommandId>,
    journal: &lib_ipc::journal::JournalWriter<EngineCommand>,
) {
    let now = Instant::now();
    let expired: Vec<CommandId> = pending.iter().filter_map(|(id, req)| (req.deadline <= now).then_some(*id)).collect();
    for id in expired {
        if let Some(req) = pending.remove(&id) {
            let _ = req.respond_to.send(Err(lib_ipc::client::ClientTransportError::Io(io::Error::new(io::ErrorKind::TimedOut, format!("{} timed out waiting for engine event", req.label)))));
            retire_journal_entry(id, journal_order, completed_journal_ids, journal);
        }
    }
}

fn flush_pending_disconnect(
    pending: &mut HashMap<CommandId, EngineRequest>,
    journal_order: &mut VecDeque<(CommandId, JournalEntry<EngineCommand>)>,
    completed_journal_ids: &mut HashSet<CommandId>,
    journal: &lib_ipc::journal::JournalWriter<EngineCommand>,
) {
    let drained: Vec<EngineRequest> = pending.drain().map(|(_, req)| req).collect();
    for req in drained {
        let command_id = req.command_id;
        let _ = req.respond_to.send(Err(disconnected_error()));
        retire_journal_entry(command_id, journal_order, completed_journal_ids, journal);
    }
}

fn retire_journal_entry(
    command_id: CommandId,
    journal_order: &mut VecDeque<(CommandId, JournalEntry<EngineCommand>)>,
    completed_journal_ids: &mut HashSet<CommandId>,
    journal: &lib_ipc::journal::JournalWriter<EngineCommand>,
) {
    if !journal_order.iter().any(|(queued_id, _)| *queued_id == command_id) {
        return;
    }
    completed_journal_ids.insert(command_id);
    truncate_completed_journal_prefix(journal_order, completed_journal_ids, journal);
}

fn truncate_completed_journal_prefix(
    journal_order: &mut VecDeque<(CommandId, JournalEntry<EngineCommand>)>,
    completed_journal_ids: &mut HashSet<CommandId>,
    journal: &lib_ipc::journal::JournalWriter<EngineCommand>,
) {
    let mut truncate_entry: Option<JournalEntry<EngineCommand>> = None;
    while let Some((command_id, entry)) = journal_order.front() {
        if !completed_journal_ids.remove(command_id) {
            break;
        }
        truncate_entry = Some(entry.clone());
        journal_order.pop_front();
    }

    if let Some(entry) = truncate_entry
        && let Err(err) = journal.truncate_through(&entry)
    {
        warn!(%err, "failed to compact engine journal");
    }
}
