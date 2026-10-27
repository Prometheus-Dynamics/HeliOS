//! A live map's save and load through the driver: a save request gives the map once, on the rising
//! edge of the graph's `save` input, and the saved bytes start a new run through its `map` input.

use eidos_daedalus::types::Bytes;
use helios_maps::wire_map_ports;

use super::live_map::{SCENE_FOCAL_PX, live_map_scene};
use super::*;
use crate::{execution::map_io::SAVED_MAP_PORT, model::MapSource};

/// The live-map golden graph as the API deploys it: `map` and `save` wired to the host.
fn live_map_document() -> String {
    let mut graph: serde_json::Value = serde_json::from_str(include_str!("../../../graphs/live-map.graph.json")).expect("the golden graph");
    assert!(wire_map_ports(&mut graph));
    graph.to_string()
}

fn live_map_workload(id: &str) -> ExecutionWorkload {
    let calibrated = calibration_context(SCENE_FOCAL_PX, "pinhole");
    let mut spec = workload(id, live_map_document(), vec![camera_binding(&calibrated)]);
    spec.plugin_requirements = vec![PluginRequirement { plugin_name: "eidos".into(), version: None }];
    spec
}

fn spawn_driver(plugins: &TestPlugins, spec: ExecutionWorkload, bus: PipelineBus) -> (WorkloadDriver, TestCamera) {
    let graph = graph::compile_workload_graph(plugins.registry(), &plugins.metadata, &spec, &["frame".to_string()], GraphSettings::default()).expect("compile the live map");
    let (sender, receiver) = test_camera();
    let frames = vec![(frame_spec("frame"), Box::new(receiver) as Box<dyn FrameSource>)];
    let driver = WorkloadDriver::start(spec, graph, frames, Arc::new(Notify::new()), crate::nt4::Nt4Sink::default(), bus).expect("start the driver");
    (driver, sender)
}

/// Sends frame `index` (a tag in view) and waits until the graph has ticked on it.
fn feed(driver: &WorkloadDriver, frames: &TestCamera, index: u64) {
    frames.send(live_map_scene(586, index, (index + 1) * 100_000_000)).expect("send a frame");
    wait_for("the frame to be processed", || driver.stats().ticks_processed == index + 1);
}

#[test]
fn a_save_request_gives_the_map_once_and_the_saved_bytes_start_a_run() {
    let plugins = TestPlugins::load();
    let bus = PipelineBus::default();
    let mut spec = live_map_workload("workload.saved-map");
    let (mut driver, frames) = spawn_driver(&plugins, spec.clone(), bus.clone());
    driver.update_camera_context(&spec);
    for index in 0..30 {
        feed(&driver, &frames, index);
    }
    assert!(bus.latest("workload.saved-map", SAVED_MAP_PORT).is_none(), "no save was asked for");

    // A new request raises `save`: the next tick saves, and that tick gives `saved_map`.
    spec.map_save = 1;
    driver.update_map(&spec);
    feed(&driver, &frames, 30);
    let (_, first) = bus.latest("workload.saved-map", SAVED_MAP_PORT).expect("the save gave saved_map");
    let bytes = first.payload.get_ref::<Bytes>().expect("saved_map is bytes").bytes.clone();
    assert_eq!(&bytes[..8], b"EIDMAP\0\0", "the map file starts with its magic");

    // No new request, no new edge: later ticks give no further save.
    for index in 31..36 {
        feed(&driver, &frames, index);
    }
    let (_, unchanged) = bus.latest("workload.saved-map", SAVED_MAP_PORT).expect("the first save stays the latest");
    assert_eq!(unchanged.sequence, first.sequence, "one request is one save");

    // Another request is another edge, and another save.
    spec.map_save = 2;
    driver.update_map(&spec);
    feed(&driver, &frames, 36);
    let (_, second) = bus.latest("workload.saved-map", SAVED_MAP_PORT).expect("the second save");
    assert!(second.sequence > first.sequence, "a new request saves again");

    // The saved bytes are a map: a run that starts from them takes them at start and ticks on.
    let dir = tempfile::tempdir().expect("a directory");
    let path = dir.path().join("saved.eidmap");
    std::fs::write(&path, &bytes).expect("write the map");
    let mut loaded = live_map_workload("workload.saved-map-loaded");
    loaded.map = Some(MapSource { id: "saved".into(), path });
    let (mut restarted, loaded_frames) = spawn_driver(&plugins, loaded.clone(), PipelineBus::default());
    restarted.update_camera_context(&loaded);
    for index in 0..10 {
        feed(&restarted, &loaded_frames, index);
    }
    assert_eq!(restarted.stats().last_error, None, "the graph took the saved map");
}

#[test]
fn a_map_the_graph_cannot_read_fails_the_start() {
    let plugins = TestPlugins::load();
    let mut spec = live_map_workload("workload.missing-map");
    spec.map = Some(MapSource { id: "gone".into(), path: "/nonexistent/helios/gone.eidmap".into() });
    let graph = graph::compile_workload_graph(plugins.registry(), &plugins.metadata, &spec, &["frame".to_string()], GraphSettings::default()).expect("compile the live map");
    let (_sender, receiver) = test_camera();
    let frames = vec![(frame_spec("frame"), Box::new(receiver) as Box<dyn FrameSource>)];
    let error =
        WorkloadDriver::start(spec, graph, frames, Arc::new(Notify::new()), crate::nt4::Nt4Sink::default(), PipelineBus::default()).err().expect("a missing map is an error, not an empty start");
    assert!(error.to_string().contains("gone.eidmap"), "{error}");
}

#[test]
fn a_graph_stored_without_the_map_ports_saves_its_map() {
    // The golden graph as Eidos generates it: no host ports for the map or the save. A graph stored
    // before HeliOS wired them must save all the same, with no redeploy.
    let plugins = TestPlugins::load();
    let calibrated = calibration_context(SCENE_FOCAL_PX, "pinhole");
    // Stored before Eidos gave the mapper its map ports, too: the mapper's port lists lack them, and
    // the compile refreshes them from the plugin (`PluginRegistry::refresh_graph_ports`).
    let mut raw: serde_json::Value = serde_json::from_str(include_str!("../../../graphs/live-map.graph.json")).expect("the golden graph");
    for node in raw["graph"]["nodes"].as_array_mut().expect("nodes") {
        if node["id"] == "eidos:mapping.live_map" {
            node["inputs"].as_array_mut().expect("inputs").retain(|port| port != "map" && port != "save");
            node["outputs"].as_array_mut().expect("outputs").retain(|port| port != SAVED_MAP_PORT);
            for key in ["inputs.map.access", "inputs.save.access", "outputs.saved_map.conditional"] {
                node["metadata"].as_object_mut().expect("metadata").remove(key);
            }
        }
    }
    let mut spec = workload("workload.stored-unwired", raw.to_string(), vec![camera_binding(&calibrated)]);
    spec.plugin_requirements = vec![PluginRequirement { plugin_name: "eidos".into(), version: None }];
    let bus = PipelineBus::default();
    let (mut driver, frames) = spawn_driver(&plugins, spec.clone(), bus.clone());
    driver.update_camera_context(&spec);
    for index in 0..10 {
        feed(&driver, &frames, index);
    }
    spec.map_save = 1;
    driver.update_map(&spec);
    feed(&driver, &frames, 10);
    let (_, saved) = bus.latest("workload.stored-unwired", SAVED_MAP_PORT).expect("the stored graph saved its map");
    let bytes = saved.payload.get_ref::<Bytes>().expect("saved_map is bytes").bytes.clone();
    assert_eq!(&bytes[..8], b"EIDMAP\0\0");
    assert_eq!(driver.stats().last_error, None);
}
