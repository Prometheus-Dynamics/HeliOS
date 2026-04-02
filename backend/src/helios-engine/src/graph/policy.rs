use super::*;
use lib_runtime_policy::{EngineExecutorBusyPolicy, ResolvedEngineGraphPolicy, HELIOS_ENGINE_GRAPH_POLICY};
use std::sync::OnceLock;

pub(super) fn graph_policy() -> &'static ResolvedEngineGraphPolicy {
    static VALUE: OnceLock<ResolvedEngineGraphPolicy> = OnceLock::new();
    VALUE.get_or_init(|| HELIOS_ENGINE_GRAPH_POLICY.resolve())
}

pub(super) fn pool_size() -> Option<usize> {
    graph_policy().pool_size
}

pub(super) fn runtime_queue_cap() -> usize {
    graph_policy().runtime_queue_cap
}

pub(super) fn dedicated_executor() -> bool {
    graph_policy().dedicated_executor
}

pub(super) fn executor_busy_behavior() -> ExecutorBusyBehavior {
    match graph_policy().executor_busy {
        EngineExecutorBusyPolicy::Drop => ExecutorBusyBehavior::Drop,
        EngineExecutorBusyPolicy::Block => ExecutorBusyBehavior::Block,
    }
}

pub(super) fn executor_busy_timeout() -> Option<Duration> {
    graph_policy().executor_busy_timeout_ms.map(Duration::from_millis)
}

pub(super) fn auto_target_roi_enabled() -> bool {
    graph_policy().auto_target_roi
}

pub(super) fn host_outputs_in_graph_enabled(plan: Option<&RuntimePlan>, gpu_plan_active: bool) -> bool {
    let requested = graph_policy().host_outputs_in_graph;
    if !requested {
        return false;
    }
    if gpu_plan_active && plan.is_some_and(super::plan_uses_gpu) {
        tracing::warn!("HELIOS_DAEDALUS_HOST_OUTPUTS_IN_GRAPH is disabled for GPU plans (stability guard)");
        return false;
    }
    true
}

pub(super) fn demand_driven_enabled(plan: Option<&RuntimePlan>, gpu_plan_active: bool) -> bool {
    let requested = graph_policy().demand_driven;
    if !requested {
        return false;
    }
    if gpu_plan_active && plan.is_some_and(super::plan_uses_gpu) {
        tracing::warn!("HELIOS_DAEDALUS_DEMAND_DRIVEN is disabled for GPU plans (stability guard)");
        return false;
    }
    true
}

pub(super) fn host_output_debug_enabled() -> bool {
    graph_policy().host_output_debug
}

pub(super) fn perf_counters_enabled() -> bool {
    cfg!(all(feature = "perf-counters", target_os = "linux")) && graph_policy().perf_counters
}

pub(super) fn pprof_enabled() -> bool {
    cfg!(feature = "pprof") && graph_policy().pprof_enabled
}

pub(super) fn background_graph_trim_interval_ms() -> u64 {
    graph_policy().background_trim_interval_ms
}

pub(super) fn active_graph_trim_interval_ms() -> u64 {
    graph_policy().active_trim_interval_ms
}

pub(super) fn host_output_sample_ttl_ms() -> u64 {
    graph_policy().host_output_sample_ttl_ms
}

pub(super) fn pprof_frames() -> u64 {
    graph_policy().pprof_frames
}

pub(super) fn pprof_duration_ms() -> Option<u64> {
    graph_policy().pprof_duration_ms
}
