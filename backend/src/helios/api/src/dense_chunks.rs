//! The dense mapper's submap chunks, kept for the UI (`eidos:dense.map`'s chunk outputs, which Eidos
//! wires to the host as `dense_mesh_chunk`, `dense_transforms` and `dense_status`). The whole mesh is no longer read: a submap's geometry is in submap-local
//! coordinates, its pose in `transforms`, and the UI draws each submap where its pose puts it.
//!
//! The store takes the engine's live lines for the dense pipelines (the relay is read only while a
//! dense pipeline is deployed: the engine serializes nothing for nobody), keeps the raw JSON of each
//! chunk and pose as it came (no re-encoding on the way in or out), and serves the changes since a
//! cursor: `GET /v1/pipelines/{id}/dense?after=<seq>&epoch=<epoch>`. The UI reads it incrementally,
//! so a second poll costs what changed, not the whole mesh.
//!
//! Bytes are bounded: the chunks held are at most `HELIOS_API_DENSE_CHUNK_BYTES` (default
//! [`DEFAULT_CHUNK_BYTES`]). Past that, the chunks changed longest ago go first, and the reader is
//! told (`removed`). The newest chunk is always kept, whatever its size. Poses are small and not
//! counted. A new epoch (the engine's mapper was rebuilt) drops everything held.

use std::{
    collections::BTreeMap,
    sync::{Mutex, MutexGuard},
    time::{Duration, Instant},
};

use axum::{
    extract::{Path, Query, State},
    http::header,
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use serde_json::value::RawValue;

use crate::{SharedState, error::ApiResult, live_outputs::LiveReceiver, routes::pipelines};

/// The bytes of submap chunks kept by default.
pub const DEFAULT_CHUNK_BYTES: usize = 16 << 20;
/// How often the deployed pipelines are checked, and the wait between reads when none are.
const POLL: Duration = Duration::from_secs(2);
/// The range of `HELIOS_API_DENSE_CHUNK_BYTES`: a chunk the size of a whole submap must fit.
pub const CHUNK_BYTES_RANGE: std::ops::RangeInclusive<usize> = (1 << 20)..=(256 << 20);

/// Whether a live line is one of the dense mapper's chunk outputs (the cheap test before a parse).
/// The dense mapper's ports: the only ones read from the relay.
const DENSE_PORTS: [&str; 3] = ["dense_mesh_chunk", "dense_transforms", "dense_status"];

fn wanted_line(line: &str) -> bool {
    line.contains("\"dense_mesh_chunk\"") || line.contains("\"dense_transforms\"") || line.contains("\"dense_status\"")
}

/// One live line's header; its value is kept as it came.
#[derive(Deserialize)]
struct Line<'a> {
    pipeline: &'a str,
    port: &'a str,
    #[serde(borrow)]
    value: &'a RawValue,
}

/// A batch of chunks (`dense_mesh_chunk`): the run's epoch and each chunk as it came.
#[derive(Deserialize)]
struct ChunkBatch<'a> {
    epoch: u64,
    #[serde(borrow)]
    chunks: Vec<&'a RawValue>,
}

/// A batch of poses (`transforms`).
#[derive(Deserialize)]
struct PoseBatch<'a> {
    epoch: u64,
    #[serde(borrow)]
    entries: Vec<&'a RawValue>,
}

/// The fields of a chunk or a pose that place it in the store.
#[derive(Deserialize)]
struct Head {
    submap: u64,
    revision: u64,
    #[serde(default)]
    kind: String,
}

/// A submap's pose, as it came.
struct Pose {
    revision: u64,
    raw: Box<RawValue>,
}

/// A submap's geometry, as it came.
struct Geometry {
    revision: u64,
    frozen: bool,
    raw: Box<RawValue>,
}

#[derive(Default)]
struct Submap {
    /// The change that last touched this submap (its entry is sent to readers behind it).
    seq: u64,
    pose: Option<Pose>,
    geometry: Option<Geometry>,
    /// Dropped for the byte cap: readers remove it, and it comes back with its next chunk.
    evicted: bool,
}

/// One pipeline's dense outputs: the current run (its epoch) and what it holds.
#[derive(Default)]
struct Run {
    epoch: Option<u64>,
    /// The last change, in sequence: readers ask for what came after their cursor.
    seq: u64,
    submaps: BTreeMap<u64, Submap>,
    status: Option<Box<RawValue>>,
    status_seq: u64,
    /// Bytes of the geometry held (what the byte cap bounds).
    geometry_bytes: usize,
}

impl Run {
    /// Starts over when `epoch` is not the run's (the mapper was rebuilt): what it held is dropped.
    fn begin(&mut self, epoch: u64) {
        if self.epoch == Some(epoch) {
            return;
        }
        self.epoch = Some(epoch);
        self.submaps.clear();
        self.status = None;
        self.geometry_bytes = 0;
        self.seq += 1;
    }

    fn bump(&mut self) -> u64 {
        self.seq += 1;
        self.seq
    }

    /// Keeps a pose if it is the newest one of its submap.
    fn put_pose(&mut self, head: &Head, raw: &RawValue) {
        if self.submaps.get(&head.submap).and_then(|entry| entry.pose.as_ref()).is_some_and(|pose| pose.revision > head.revision) {
            return;
        }
        let seq = self.bump();
        let entry = self.submaps.entry(head.submap).or_default();
        entry.pose = Some(Pose { revision: head.revision, raw: raw.to_owned() });
        entry.seq = seq;
    }

    /// Keeps a chunk unless a newer one, or a frozen one, is held for its submap.
    fn put_geometry(&mut self, head: &Head, raw: &RawValue) {
        let frozen = head.kind == "frozen";
        let held = self.submaps.get(&head.submap).and_then(|entry| entry.geometry.as_ref());
        let supersedes = match held {
            None => true,
            Some(_) if frozen => true,
            Some(held) if held.frozen => false,
            Some(held) => head.revision >= held.revision,
        };
        if !supersedes {
            return;
        }
        let seq = self.bump();
        let entry = self.submaps.entry(head.submap).or_default();
        if let Some(old) = entry.geometry.take() {
            self.geometry_bytes -= old.raw.get().len();
        }
        self.geometry_bytes += raw.get().len();
        entry.geometry = Some(Geometry { revision: head.revision, frozen, raw: raw.to_owned() });
        entry.evicted = false;
        entry.seq = seq;
    }

    /// Drops the geometry changed longest ago until the held bytes fit `cap` (the newest one stays).
    fn evict(&mut self, cap: usize) {
        while self.geometry_bytes > cap {
            let held = self.submaps.values().filter(|entry| entry.geometry.is_some()).count();
            let oldest = self.submaps.iter().filter(|(_, entry)| entry.geometry.is_some()).min_by_key(|(_, entry)| entry.seq).map(|(id, _)| *id);
            // The newest chunk stays, whatever its size.
            let Some(id) = oldest.filter(|_| held > 1) else { return };
            let seq = self.bump();
            let entry = self.submaps.get_mut(&id).expect("the submap is held");
            if let Some(geometry) = entry.geometry.take() {
                self.geometry_bytes -= geometry.raw.get().len();
            }
            entry.evicted = true;
            entry.seq = seq;
        }
    }
}

/// The dense outputs of every pipeline, by pipeline id.
pub struct DenseChunks {
    cap: usize,
    runs: Mutex<BTreeMap<String, Run>>,
}

/// One submap's entry in a read.
#[derive(Serialize)]
struct OutSubmap<'a> {
    submap: u64,
    removed: bool,
    transform: Option<&'a RawValue>,
    chunk: Option<&'a RawValue>,
}

/// What a reader gets: the run, the cursor to ask from next, and the submaps changed since its cursor.
#[derive(Serialize)]
struct Out<'a> {
    epoch: Option<u64>,
    seq: u64,
    /// The reader's epoch is not the run's: its cursor is void and this is the run from the start.
    reset: bool,
    status: Option<&'a RawValue>,
    submaps: Vec<OutSubmap<'a>>,
}

impl DenseChunks {
    /// A store keeping at most `cap` bytes of submap geometry.
    pub fn new(cap: usize) -> Self {
        Self { cap, runs: Mutex::default() }
    }

    fn runs(&self) -> MutexGuard<'_, BTreeMap<String, Run>> {
        self.runs.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Takes the chunk outputs in a block of live lines (one or more, newline-delimited).
    pub fn ingest_lines(&self, text: &str) {
        for line in text.lines().filter(|line| wanted_line(line)) {
            let Ok(parsed) = serde_json::from_str::<Line>(line) else { continue };
            let pipeline = pipelines::pipeline_id(parsed.pipeline);
            self.ingest(&pipeline, parsed.port, parsed.value);
        }
    }

    fn ingest(&self, pipeline: &str, port: &str, value: &RawValue) {
        let mut runs = self.runs();
        let run = runs.entry(pipeline.to_string()).or_default();
        match port {
            "dense_status" => {
                let Ok(head) = serde_json::from_str::<EpochOnly>(value.get()) else { return };
                run.begin(head.epoch);
                run.status = Some(value.to_owned());
                run.status_seq = run.bump();
            }
            "dense_transforms" => {
                let Ok(batch) = serde_json::from_str::<PoseBatch>(value.get()) else { return };
                run.begin(batch.epoch);
                for raw in batch.entries {
                    if let Ok(head) = serde_json::from_str::<Head>(raw.get()) {
                        run.put_pose(&head, raw);
                    }
                }
            }
            "dense_mesh_chunk" => {
                let Ok(batch) = serde_json::from_str::<ChunkBatch>(value.get()) else { return };
                run.begin(batch.epoch);
                for raw in batch.chunks {
                    if let Ok(head) = serde_json::from_str::<Head>(raw.get()) {
                        run.put_geometry(&head, raw);
                    }
                }
                run.evict(self.cap);
            }
            _ => {}
        }
    }

    /// Forgets everything held (no dense pipeline is deployed any more).
    pub fn clear(&self) {
        self.runs().clear();
    }

    /// The changes to a pipeline's dense outputs since `after` (its cursor), as the JSON the route sends.
    pub fn read(&self, pipeline: &str, after: u64, epoch: Option<u64>) -> Vec<u8> {
        let runs = self.runs();
        let empty = Run::default();
        let run = runs.get(pipeline).unwrap_or(&empty);
        let reset = epoch.is_some_and(|wanted| run.epoch != Some(wanted));
        let after = if reset { 0 } else { after };
        let submaps = run
            .submaps
            .iter()
            .filter(|(_, entry)| entry.seq > after)
            .map(|(id, entry)| OutSubmap { submap: *id, removed: entry.evicted, transform: entry.pose.as_ref().map(|pose| &*pose.raw), chunk: entry.geometry.as_ref().map(|geometry| &*geometry.raw) })
            .collect();
        let out = Out { epoch: run.epoch, seq: run.seq, reset, status: if run.status_seq > after { run.status.as_deref() } else { None }, submaps };
        serde_json::to_vec(&out).unwrap_or_else(|_| b"{}".to_vec())
    }

    /// Bytes of submap geometry held, over all pipelines (for the device's own accounting).
    pub fn geometry_bytes(&self) -> usize {
        self.runs().values().map(|run| run.geometry_bytes).sum()
    }
}

/// Only the epoch of a `status` value.
#[derive(Deserialize)]
struct EpochOnly {
    epoch: u64,
}

/// The query of `GET /v1/pipelines/{id}/dense`.
#[derive(Debug, Default, Deserialize)]
pub struct DenseQuery {
    /// The cursor the reader holds (its last `seq`); 0 for everything.
    pub after: Option<u64>,
    /// The epoch the reader holds; when it is not the run's, the reader gets the run from the start.
    pub epoch: Option<u64>,
}

/// `GET /v1/pipelines/{id}/dense?after=&epoch=`: the dense mapper's chunk changes since the cursor.
pub async fn read_route(State(state): State<SharedState>, Path(id): Path<String>, Query(query): Query<DenseQuery>) -> ApiResult<Response> {
    let view = state.orion.view().await?;
    let pipeline = pipelines::find_workload(&view, &id).map(|w| pipelines::pipeline_id(w.workload_id.as_str())).unwrap_or(id);
    let body = state.dense.read(&pipeline, query.after.unwrap_or(0), query.epoch);
    Ok(([(header::CONTENT_TYPE, "application/json")], body).into_response())
}

/// Takes the dense pipelines' chunk outputs from the live relay into `state.dense`, for as long as a
/// dense pipeline is deployed (the relay is read, and the engine serializes, only while it is).
pub fn spawn(state: SharedState) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut receiver: Option<LiveReceiver> = None;
        let mut next_check = Instant::now();
        loop {
            if Instant::now() >= next_check {
                next_check = Instant::now() + POLL;
                let wanted = match state.orion.view().await {
                    Ok(view) => !pipelines::dense_map_pipelines(&view).is_empty(),
                    Err(_) => receiver.is_some(),
                };
                if wanted && receiver.is_none() {
                    receiver = Some(state.live.subscribe_ports(&DENSE_PORTS));
                } else if !wanted && receiver.take().is_some() {
                    state.dense.clear();
                }
            }
            let Some(rx) = receiver.as_mut() else {
                tokio::time::sleep(POLL).await;
                continue;
            };
            let mut closed = false;
            tokio::select! {
                chunk = rx.recv() => match chunk {
                    Ok(text) => state.dense.ingest_lines(&text),
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(missed)) => {
                        tracing::warn!(missed, "dense chunks: the live relay ran ahead of the store; some chunks were missed");
                    }
                    Err(_) => closed = true,
                },
                () = tokio::time::sleep(POLL) => {}
            }
            if closed {
                receiver = None;
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A live line of `port` for `pipeline`, as the engine writes it.
    fn line(pipeline: &str, port: &str, value: &str) -> String {
        format!("{{\"pipeline\":\"{pipeline}\",\"port\":\"{port}\",\"observed_at_ms\":1,\"frame_ts_ns\":null,\"value\":{value}}}\n")
    }

    /// A chunk of `submap` at `revision`, `kind`, carrying `bytes` of payload.
    fn chunk(submap: u64, revision: u64, kind: &str, bytes: usize) -> String {
        format!(
            "{{\"submap\":{submap},\"revision\":{revision},\"kind\":\"{kind}\",\"vertices\":1,\"triangles\":0,\"positions\":\"{}\",\"normals\":\"\",\"grey\":\"\",\"indices\":\"\"}}",
            "A".repeat(bytes)
        )
    }

    fn batch(epoch: u64, chunks: &[String]) -> String {
        format!("{{\"frame\":1,\"t_ns\":2,\"epoch\":{epoch},\"chunks\":[{}]}}", chunks.join(","))
    }

    fn poses(epoch: u64, entries: &[(u64, u64)]) -> String {
        let items: Vec<String> =
            entries.iter().map(|(submap, revision)| format!("{{\"submap\":{submap},\"revision\":{revision},\"pose\":{{\"translation\":{{\"x\":{submap}.0,\"y\":0,\"z\":0}}}}}}")).collect();
        format!("{{\"frame\":1,\"t_ns\":2,\"epoch\":{epoch},\"entries\":[{}]}}", items.join(","))
    }

    fn read(store: &DenseChunks, after: u64, epoch: Option<u64>) -> serde_json::Value {
        serde_json::from_slice(&store.read("dense", after, epoch)).expect("the read is JSON")
    }

    #[test]
    fn chunks_and_poses_are_read_incrementally_by_cursor() {
        let store = DenseChunks::new(DEFAULT_CHUNK_BYTES);
        store.ingest_lines(&line("dense", "dense_transforms", &poses(7, &[(0, 0)])));
        store.ingest_lines(&line("dense", "dense_mesh_chunk", &batch(7, &[chunk(0, 0, "frozen", 8)])));
        let first = read(&store, 0, None);
        assert_eq!(first["epoch"], 7);
        assert_eq!(first["submaps"].as_array().expect("submaps").len(), 1);
        assert_eq!(first["submaps"][0]["chunk"]["kind"], "frozen");
        assert_eq!(first["submaps"][0]["transform"]["submap"], 0);
        let cursor = first["seq"].as_u64().expect("seq");

        assert_eq!(read(&store, cursor, Some(7))["submaps"].as_array().expect("submaps").len(), 0, "nothing new since the cursor");
        store.ingest_lines(&line("dense", "dense_mesh_chunk", &batch(7, &[chunk(1, 1, "current", 8)])));
        let next = read(&store, cursor, Some(7));
        let changed: Vec<u64> = next["submaps"].as_array().expect("submaps").iter().map(|s| s["submap"].as_u64().expect("id")).collect();
        assert_eq!(changed, [1], "only the submap that changed is sent");
    }

    #[test]
    fn a_frozen_chunk_supersedes_the_current_one_and_a_stale_one_is_ignored() {
        let store = DenseChunks::new(DEFAULT_CHUNK_BYTES);
        store.ingest_lines(&line("dense", "dense_mesh_chunk", &batch(1, &[chunk(0, 3, "current", 4)])));
        store.ingest_lines(&line("dense", "dense_mesh_chunk", &batch(1, &[chunk(0, 0, "frozen", 6)])));
        store.ingest_lines(&line("dense", "dense_mesh_chunk", &batch(1, &[chunk(0, 9, "current", 4)])));
        let out = read(&store, 0, None);
        assert_eq!(out["submaps"][0]["chunk"]["kind"], "frozen", "frozen supersedes whatever the current revision");
        store.ingest_lines(&line("dense", "dense_transforms", &poses(1, &[(5, 2)])));
        store.ingest_lines(&line("dense", "dense_transforms", &poses(1, &[(5, 1)])));
        let out = read(&store, 0, None);
        let entry = out["submaps"].as_array().expect("submaps").iter().find(|s| s["submap"] == 5).expect("pose kept");
        assert_eq!(entry["transform"]["revision"], 2, "an older pose does not replace a newer one");
    }

    #[test]
    fn a_new_epoch_starts_the_run_over_and_a_reader_on_the_old_one_is_told() {
        let store = DenseChunks::new(DEFAULT_CHUNK_BYTES);
        store.ingest_lines(&line("dense", "dense_mesh_chunk", &batch(1, &[chunk(0, 0, "frozen", 4)])));
        let old = read(&store, 0, None);
        store.ingest_lines(&line("dense", "dense_mesh_chunk", &batch(2, &[chunk(4, 0, "frozen", 4)])));
        let reader_on_old = read(&store, old["seq"].as_u64().expect("seq"), Some(1));
        assert_eq!(reader_on_old["reset"], true, "the reader's epoch is no longer the run's");
        assert_eq!(reader_on_old["epoch"], 2);
        assert_eq!(reader_on_old["submaps"].as_array().expect("submaps").len(), 1, "from the start, without the old run");
        assert_eq!(reader_on_old["submaps"][0]["submap"], 4);
    }

    #[test]
    fn the_byte_cap_drops_the_oldest_chunks_and_says_so_newest_always_kept() {
        let store = DenseChunks::new(100);
        for submap in 0..4 {
            store.ingest_lines(&line("dense", "dense_mesh_chunk", &batch(1, &[chunk(submap, 0, "frozen", 40)])));
        }
        let out = read(&store, 0, None);
        let held: Vec<(u64, bool)> = out["submaps"].as_array().expect("submaps").iter().map(|s| (s["submap"].as_u64().expect("id"), s["removed"] == true)).collect();
        assert!(held.contains(&(3, false)), "the newest is kept: {held:?}");
        assert!(held.iter().any(|(_, removed)| *removed), "an evicted submap is reported as removed: {held:?}");
        assert!(!held.iter().any(|(id, removed)| *id == 0 && !removed), "the oldest went first");
    }

    #[test]
    fn a_chunk_bigger_than_the_cap_is_still_the_newest_and_kept() {
        let store = DenseChunks::new(10);
        store.ingest_lines(&line("dense", "dense_mesh_chunk", &batch(1, &[chunk(0, 0, "frozen", 200)])));
        assert!(store.geometry_bytes() > 10, "the one chunk is kept whatever its size");
    }

    #[test]
    fn status_is_kept_and_sent_once_per_change() {
        let store = DenseChunks::new(DEFAULT_CHUNK_BYTES);
        store.ingest_lines(&line("dense", "dense_status", r#"{"frame":3,"t_ns":4,"epoch":9,"keyframes":5,"submaps":2,"bytes":100}"#));
        let out = read(&store, 0, None);
        assert_eq!(out["status"]["keyframes"], 5);
        let cursor = out["seq"].as_u64().expect("seq");
        assert!(read(&store, cursor, None)["status"].is_null(), "unchanged status is not sent again");
    }

    #[test]
    fn lines_of_other_ports_and_pipelines_are_separate() {
        let store = DenseChunks::new(DEFAULT_CHUNK_BYTES);
        store.ingest_lines(&line("other", "dense_mesh_chunk", &batch(1, &[chunk(0, 0, "frozen", 4)])));
        store.ingest_lines(&line("dense", "detections", r#"{"detections":[]}"#));
        assert!(read(&store, 0, None)["submaps"].as_array().expect("submaps").is_empty());
        assert!(!store.read("other", 0, None).is_empty());
    }
}
