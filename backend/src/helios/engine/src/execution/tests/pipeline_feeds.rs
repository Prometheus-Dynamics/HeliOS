//! Pipeline inputs (`source: "pipeline"`) through the frame driver: a producer's output reaches a
//! consumer graph for the same frame, a restarted producer's values replace the old ones, a
//! consumer with no producer keeps ticking, and a producer that does not reach a frame costs the
//! consumer at most its wait.

use super::*;
use crate::{execution::pipeline_inputs::FeedState, model::PipelineInputSource};

const MS: u64 = 1_000_000;

/// A graph that feeds the frame and its `context` input to `frame_context`, whose output is the
/// host's `result`: the producer's context is its `imu`, the consumer's its `upstream`.
fn fusion_document(plugins: &TestPlugins, context_input: &str) -> String {
    let fusion = EngineTestPlugin::new().frame_context.clone().alias("fusion");
    let graph = plugins
        .registry()
        .graph_builder()
        .expect("graph builder")
        .host_bridge("host")
        .node(&fusion)
        .connect(&host_port("host", "camera"), &fusion.inputs.frame)
        .connect(&host_port("host", context_input), &fusion.inputs.context)
        .connect(&fusion.outputs.out, &host_port("host", "result"))
        .build();
    plugins.registry().graph_document(graph).to_json().expect("document json")
}

fn producer_workload(plugins: &TestPlugins) -> ExecutionWorkload {
    workload("pipeline.producer", fusion_document(plugins, "imu"), Vec::new())
}

/// The consumer takes the producer's `result` as its `upstream` input, with the frame of its own
/// camera (`camera`).
fn consumer_workload(plugins: &TestPlugins, max_wait_ms: u32) -> ExecutionWorkload {
    let upstream = ExecutionBinding { input: "upstream".into(), pipeline: Some(PipelineInputSource { pipeline: "producer".into(), port: "result".into(), max_wait_ms }), ..ExecutionBinding::default() };
    workload("pipeline.consumer", fusion_document(plugins, "upstream"), vec![upstream])
}

/// Start a frame-driven workload on `bus`, its frames from the returned camera.
fn start(plugins: &TestPlugins, bus: &PipelineBus, workload: ExecutionWorkload) -> (WorkloadDriver, TestCamera) {
    let graph = graph::compile_workload_graph(plugins.registry(), &plugins.metadata, &workload, &["camera".to_string()], GraphSettings::default()).expect("compile graph");
    let (sender, receiver) = test_camera();
    let driver = WorkloadDriver::start(workload, graph, vec![(frame_spec("camera"), Box::new(receiver) as Box<dyn FrameSource>)], Arc::new(Notify::new()), crate::nt4::Nt4Sink::default(), bus.clone())
        .expect("start driver");
    (driver, sender)
}

/// The producer with its `imu` context set.
fn start_producer(plugins: &TestPlugins, bus: &PipelineBus, imu: &str) -> (WorkloadDriver, TestCamera) {
    let (mut producer, frames) = start(plugins, bus, producer_workload(plugins));
    producer.update_context(vec![ResourceInput { input: "imu".into(), revision: imu.into(), payload: imu.into() }]);
    (producer, frames)
}

fn result(driver: &mut WorkloadDriver) -> String {
    driver.snapshot().1.into_iter().find(|artifact| artifact.kind == "host_output:result").and_then(|artifact| artifact.message).expect("result")
}

#[test]
fn a_producer_output_reaches_the_consumer_graph_for_the_same_frame() {
    let plugins = TestPlugins::load();
    let bus = PipelineBus::default();
    let (producer, producer_frames) = start_producer(&plugins, &bus, "imu-a");
    let (mut consumer, consumer_frames) = start(&plugins, &bus, consumer_workload(&plugins, 5_000));

    // The consumer's frame goes first: it waits for the producer to finish the same frame.
    consumer_frames.send(flat_frame(8, 8, 200 * MS)).expect("send consumer frame");
    producer_frames.send(flat_frame(8, 8, 200 * MS)).expect("send producer frame");
    wait_for("the producer's frame", || producer.stats().ticks_processed == 1);
    wait_for("the consumer's frame", || consumer.stats().ticks_processed == 1);

    let value = result(&mut consumer);
    assert!(value.contains("imu-a") && value.contains(&format!("\"timestamp\":{}", 200 * MS)), "{value}");
    let feed = consumer.stats().pipeline_inputs.remove(0);
    assert_eq!((feed.state, feed.aligned_ticks, feed.lagged_ticks), (FeedState::Aligned, 1, 0), "{feed:?}");
    assert_eq!(feed.pipeline, "producer");
    assert_eq!(consumer.stats().last_error, None);
}

#[test]
fn a_restarted_producer_is_a_new_run_and_its_new_values_reach_the_consumer() {
    let plugins = TestPlugins::load();
    let bus = PipelineBus::default();
    let (producer, producer_frames) = start_producer(&plugins, &bus, "imu-a");
    let (mut consumer, consumer_frames) = start(&plugins, &bus, consumer_workload(&plugins, 5_000));
    consumer_frames.send(flat_frame(8, 8, 200 * MS)).expect("send consumer frame");
    producer_frames.send(flat_frame(8, 8, 200 * MS)).expect("send producer frame");
    wait_for("the first pair", || consumer.stats().ticks_processed == 1);
    assert!(result(&mut consumer).contains("imu-a"));

    // The producer stops: its values are withdrawn. The consumer ticks on, with the value it has
    // (a held input keeps its last value), and says the producer is missing.
    drop(producer);
    consumer_frames.send(flat_frame(8, 8, 300 * MS)).expect("send consumer frame");
    wait_for("the frame without the producer", || consumer.stats().ticks_processed == 2);
    let value = result(&mut consumer);
    assert!(value.contains("imu-a") && value.contains(&format!("\"timestamp\":{}", 300 * MS)), "{value}");
    let feed = consumer.stats().pipeline_inputs.remove(0);
    assert_eq!(feed.state, FeedState::Missing, "{feed:?}");
    assert_eq!(feed.missing_ticks, 1, "{feed:?}");

    // It restarts with new values: the consumer takes them for the next frame.
    let (restarted, restarted_frames) = start_producer(&plugins, &bus, "imu-b");
    consumer_frames.send(flat_frame(8, 8, 400 * MS)).expect("send consumer frame");
    restarted_frames.send(flat_frame(8, 8, 400 * MS)).expect("send producer frame");
    wait_for("the restarted producer's frame", || restarted.stats().ticks_processed == 1);
    wait_for("the frame after the restart", || consumer.stats().ticks_processed == 3);
    let value = result(&mut consumer);
    assert!(value.contains("imu-b") && value.contains(&format!("\"timestamp\":{}", 400 * MS)), "{value}");
    assert_eq!(consumer.stats().pipeline_inputs[0].state, FeedState::Aligned);
}

#[test]
fn a_consumer_without_a_producer_ticks_on_its_frames_and_says_missing() {
    let plugins = TestPlugins::load();
    let bus = PipelineBus::default();
    // The producer named is not running: the frame must not wait for it.
    let (consumer, frames) = start(&plugins, &bus, consumer_workload(&plugins, 5_000));
    let started = Instant::now();
    frames.send(flat_frame(8, 8, 100 * MS)).expect("send frame");
    wait_for("the frame", || consumer.stats().ticks_processed + consumer.stats().ticks_failed == 1);
    assert!(started.elapsed() < Duration::from_secs(2), "it waited for a producer that is not there: {:?}", started.elapsed());
    let feed = consumer.stats().pipeline_inputs.remove(0);
    assert_eq!((feed.state, feed.missing_ticks), (FeedState::Missing, 1), "{feed:?}");
}

#[test]
fn a_producer_that_does_not_reach_the_frame_costs_the_consumer_at_most_its_wait() {
    let plugins = TestPlugins::load();
    let bus = PipelineBus::default();
    let (producer, producer_frames) = start_producer(&plugins, &bus, "imu-a");
    let (mut consumer, consumer_frames) = start(&plugins, &bus, consumer_workload(&plugins, 150));
    producer_frames.send(flat_frame(8, 8, 200 * MS)).expect("send producer frame");
    wait_for("the producer's first frame", || producer.stats().ticks_processed == 1);

    // The producer never gets the frame at 300 ms: the consumer waits its 150 ms, then ticks with
    // the producer's latest value, and the tick says it is lagged by the frame gap.
    let started = Instant::now();
    consumer_frames.send(flat_frame(8, 8, 300 * MS)).expect("send consumer frame");
    wait_for("the consumer's frame", || consumer.stats().ticks_processed == 1);
    let waited = started.elapsed();
    assert!(waited >= Duration::from_millis(150), "the wait was spent: {waited:?}");
    assert!(waited < Duration::from_secs(5), "and bounded: {waited:?}");
    let feed = consumer.stats().pipeline_inputs.remove(0);
    assert_eq!(feed.state, FeedState::Lagged, "{feed:?}");
    assert_eq!(feed.lagged_ticks, 1, "{feed:?}");
    let lag = feed.lag_ms.expect("lag");
    assert!((lag - 100.0).abs() < 1e-6, "100 ms behind the producer's frame: {lag}");
    assert!(result(&mut consumer).contains("imu-a"));
}
