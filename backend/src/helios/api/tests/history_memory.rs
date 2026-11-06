//! A deploy's revision history stays out of the heap. A history is 20 revisions of a graph of
//! hundreds of KB; read as a `serde_json::Value` tree on every deploy it took 1.5 -> 73 MiB of brk
//! heap on the CM5, held for good. Allocations are counted here, so this is its own test binary:
//! nothing else may run in the process while one is measured.

// A counting allocator is the one thing here that needs it.
#![allow(unsafe_code)]

use std::{
    alloc::{GlobalAlloc, Layout, System},
    sync::atomic::{AtomicUsize, Ordering},
};

use helios_api::store::{MAX_REVISIONS, Store};

struct Counting;

static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);
static LIVE_BYTES: AtomicUsize = AtomicUsize::new(0);
static PEAK_BYTES: AtomicUsize = AtomicUsize::new(0);

// SAFETY: every call is passed on to `System` unchanged; the rest only counts.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        let live = LIVE_BYTES.fetch_add(layout.size(), Ordering::Relaxed) + layout.size();
        PEAK_BYTES.fetch_max(live, Ordering::Relaxed);
        // SAFETY: the caller's layout, as `GlobalAlloc::alloc` requires.
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        LIVE_BYTES.fetch_sub(layout.size(), Ordering::Relaxed);
        // SAFETY: `ptr` came from `alloc` with this layout.
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static COUNTING: Counting = Counting;

/// A spec shaped like a real graph: thousands of small objects, `bytes` of JSON or a little more.
fn spec(bytes: usize) -> serde_json::Value {
    let node = |n: usize| serde_json::json!({ "id": format!("node-{n}"), "x": n as f64 * 0.5, "ports": [{ "name": "in", "ty": "frame" }, { "name": "out", "ty": "tags" }] });
    let per_node = node(0).to_string().len();
    serde_json::json!({ "name": "default", "graph": { "nodes": (0..bytes / per_node + 1).map(node).collect::<Vec<_>>() } })
}

/// What `work` costs: allocations made, and the most bytes it held at once.
async fn measured<T>(work: impl Future<Output = T>) -> (T, usize, usize) {
    let (before, live) = (ALLOCATIONS.load(Ordering::Relaxed), LIVE_BYTES.load(Ordering::Relaxed));
    PEAK_BYTES.store(live, Ordering::Relaxed);
    let result = work.await;
    (result, ALLOCATIONS.load(Ordering::Relaxed) - before, PEAK_BYTES.load(Ordering::Relaxed) - live)
}

#[tokio::test(flavor = "current_thread")]
async fn a_deploy_reads_and_writes_the_history_without_a_json_tree() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = Store::new(dir.path().to_path_buf());
    let spec = spec(190 * 1024);
    for revision in 1..=MAX_REVISIONS as u64 {
        store.push_revision("default", revision, &spec, 0).await.expect("push");
    }
    let file = dir.path().join("pipelines/default.json");
    let text = std::fs::read(&file).expect("history");

    // The control: the same file as a tree, so the numbers below are known to mean something.
    let (tree, tree_allocations, tree_peak) = measured(async { serde_json::from_slice::<serde_json::Value>(&text).expect("tree") }).await;
    drop(tree);

    let (_, allocations, peak) = measured(store.push_revision("default", MAX_REVISIONS as u64 + 1, &spec, 0)).await;
    println!("tree: {tree_allocations} allocations, {tree_peak} bytes; deploy: {allocations} allocations, {peak} bytes; file {} bytes", text.len());
    // One deploy holds the history's specs (the file's size) and its own; the file is streamed in
    // and out, so its text is never held next to them.
    assert!(tree_allocations > 100_000, "the control is not a real tree: {tree_allocations} allocations");
    assert!(allocations < 1_000, "{allocations} allocations in a deploy's history write");
    assert!(allocations * 100 < tree_allocations, "{allocations} allocations against the tree's {tree_allocations}");
    assert!(peak < text.len() * 3 / 2, "{peak} bytes held for a {} byte history", text.len());
    assert!(peak * 2 < tree_peak, "{peak} bytes held against the tree's {tree_peak}");

    // A deploy's revision number reads the history without holding it.
    let (next, allocations, peak) = measured(store.next_revision("default")).await;
    println!("next revision: {allocations} allocations, {peak} bytes");
    assert_eq!(next.expect("next"), MAX_REVISIONS as u64 + 2);
    assert!(peak < 256 * 1024, "{peak} bytes held to read a revision number from a {} byte history", text.len());
}
