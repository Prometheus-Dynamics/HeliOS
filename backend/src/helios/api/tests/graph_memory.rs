//! A pipeline's graph, as a view holds it for the requests that show the pipeline, is the text
//! as stored and an outline of its nodes, not a `serde_json::Value` tree: the tree of an
//! AprilTag pipeline's graph is several times its text, and a view keeps one per pipeline for as
//! long as the state does not change. Allocations are counted here, so this is its own test
//! binary.

// A counting allocator is the one thing here that needs it.
#![allow(unsafe_code)]

use std::{
    alloc::{GlobalAlloc, Layout, System},
    sync::atomic::{AtomicUsize, Ordering},
};

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

fn live_after<T>(make: impl FnOnce() -> T) -> (T, usize) {
    let before = LIVE_BYTES.load(Ordering::Relaxed);
    let value = make();
    (value, LIVE_BYTES.load(Ordering::Relaxed).saturating_sub(before))
}

#[test]
fn a_held_graph_costs_about_its_text_not_a_tree() {
    let template = helios_api::startup::GRAPHS.iter().find(|(id, _)| *id == "apriltag-36h11-map-dense").expect("template").1;
    // As a pipeline stores it: the registry metadata stripped.
    let mut document: serde_json::Value = serde_json::from_str(template).expect("json");
    helios_api::routes::pipelines::strip_registry_metadata(&mut document);
    let text = serde_json::to_string(&document).expect("text");
    drop(document);

    let (tree, tree_bytes) = live_after(|| serde_json::from_str::<serde_json::Value>(&text).expect("tree"));
    drop(tree);
    let (graph, graph_bytes) = live_after(|| helios_api::orion::Graph::read(&text).expect("graph"));
    let outline_bytes = graph_bytes.saturating_sub(text.len());
    println!("graph text {} B; held as a tree {tree_bytes} B, as text and outline {graph_bytes} B (outline {outline_bytes} B)", text.len());
    assert!(graph.outline()["graph"]["nodes"].as_array().is_some_and(|nodes| !nodes.is_empty()));
    assert!(graph_bytes * 5 < tree_bytes, "text and outline {graph_bytes} B vs tree {tree_bytes} B");
}
