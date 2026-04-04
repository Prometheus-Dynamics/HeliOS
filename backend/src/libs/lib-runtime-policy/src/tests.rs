#![allow(unsafe_code)]

use super::{
    EngineExecutorBusyPolicy, HELIOS_API_DATA_ROOT_POLICY, HELIOS_API_HARDWARE_READ_MODEL_POLICY, HELIOS_API_LIGHTING_TEMPLATES_POLICY, HELIOS_API_LOCALIZATION_POLICY, HELIOS_API_LOG_SOURCES_POLICY,
    HELIOS_API_SERVER_POLICY, HELIOS_API_STARTUP_CACHE_WARM_POLICY, HELIOS_API_STREAMS_POLICY, HELIOS_API_SYSTEM_READ_MODEL_POLICY, HELIOS_API_TOKIO_POLICY, HELIOS_BOOT_BUTTON_DAEMON_POLICY,
    HELIOS_DAEDALUS_RUNTIME_POLICY, HELIOS_DIAGNOSTICS_DAEMON_POLICY, HELIOS_DNS_POLICY, HELIOS_ENGINE_CRASH_GUARD_POLICY, HELIOS_ENGINE_GRAPH_POLICY, HELIOS_ENGINE_IPC_POLICY,
    HELIOS_ENGINE_RECORDING_POLICY, HELIOS_ENGINE_STREAM_RUNTIME_POLICY, HELIOS_ENGINE_TOKIO_POLICY, HELIOS_I2C_INVENTORY_POLICY, HELIOS_IMU_FUSION_POLICY, HELIOS_IMU_RUNTIME_POLICY,
    HELIOS_IPC_JOURNAL_POLICY, HELIOS_LOG_FILTER_POLICY, HELIOS_PERIPHERALS_POWER_POLICY, HELIOS_PERIPHERALS_SERVICE_POLICY, HELIOS_PERIPHERALS_TOKIO_POLICY, HELIOS_RESOURCE_GUARD_POLICY,
    HELIOS_SHADOW_RECORD_DATA_ROOT_POLICY, HELIOS_STYX_CAPTURE_TUNABLES_POLICY, HELIOS_UPDATER_FILESYSTEM_POLICY, HELIOS_USB_RECOVERY_DAEMON_POLICY, PersistentDirPolicy, PlatformFamily,
    classify_platform_family,
};
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

fn env_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(())).lock().expect("env lock poisoned")
}

#[test]
fn api_server_policy_defaults_match_expected_values() {
    let resolved = HELIOS_API_SERVER_POLICY.resolve();
    assert_eq!(resolved.bind_addr.to_string(), "0.0.0.0:5800");
}

#[test]
fn api_server_policy_prefers_namespaced_bind_override() {
    let _lock = env_lock();
    unsafe {
        std::env::set_var("HELIOS_API__SERVER__BIND", "127.0.0.1:5812");
    }
    let resolved = HELIOS_API_SERVER_POLICY.resolve();
    assert_eq!(resolved.bind_addr.to_string(), "127.0.0.1:5812");
    unsafe {
        std::env::remove_var("HELIOS_API__SERVER__BIND");
    }
}

#[test]
fn api_lighting_templates_policy_prefers_namespaced_env_override() {
    let _lock = env_lock();
    unsafe {
        std::env::set_var("HELIOS_API_LIGHTING_TEMPLATE_DIR", "/tmp/helios-lighting-templates");
    }
    let resolved = HELIOS_API_LIGHTING_TEMPLATES_POLICY.resolve(None);
    assert_eq!(resolved.template_dir, PathBuf::from("/tmp/helios-lighting-templates"));
    unsafe {
        std::env::remove_var("HELIOS_API_LIGHTING_TEMPLATE_DIR");
    }
}

#[test]
fn boot_button_daemon_policy_defaults_match_expected_values() {
    let resolved = HELIOS_BOOT_BUTTON_DAEMON_POLICY.resolve();
    assert_eq!(resolved.key_tokens, vec!["KEY_RESTART", "KEY_CONFIG", "KEY_POWER"]);
    assert_eq!(resolved.hold_duration.as_secs(), 5);
    assert_eq!(resolved.cooldown.as_secs(), 15);
    assert_eq!(resolved.device_hint, None);
    assert_eq!(resolved.reset_command, "/usr/local/bin/helios-network-reset.sh");
}

#[test]
fn usb_recovery_daemon_policy_defaults_match_expected_values() {
    let resolved = HELIOS_USB_RECOVERY_DAEMON_POLICY.resolve();
    assert_eq!(resolved.ports, vec!["/dev/ttyGS0", "/dev/ttyGS1"]);
    assert_eq!(resolved.baud, 115_200);
    assert_eq!(resolved.staging_dir, PathBuf::from("/var/lib/helios/usb-recovery"));
    assert_eq!(resolved.updater_socket, PathBuf::from("/run/helios/updater.sock"));
    assert_eq!(resolved.updater_journal_path, PathBuf::from("/var/lib/helios/journal/ipc/updater-usb-recoveryd.journal"));
}

#[test]
fn diagnostics_daemon_policy_prefers_namespaced_override() {
    let _lock = env_lock();
    unsafe {
        std::env::set_var("HELIOS_DIAGNOSTICS_KEEP", "16");
        std::env::set_var("HELIOS_DIAGNOSTICS_MAX_MB", "128");
    }
    let resolved = HELIOS_DIAGNOSTICS_DAEMON_POLICY.resolve();
    assert_eq!(resolved.keep, 16);
    assert_eq!(resolved.max_mb, Some(128));
    unsafe {
        std::env::remove_var("HELIOS_DIAGNOSTICS_KEEP");
        std::env::remove_var("HELIOS_DIAGNOSTICS_MAX_MB");
    }
}

#[test]
fn api_runtime_policy_defaults_match_expected_values() {
    let resolved = HELIOS_API_TOKIO_POLICY.resolve();
    assert_eq!(resolved.worker_threads, 2);
    assert_eq!(resolved.max_blocking_threads, 4);
    assert_eq!(resolved.thread_stack_bytes, Some(1_048_576));
    assert_eq!(resolved.blocking_keep_alive.map(|value| value.as_millis()), Some(500));
}

#[test]
fn engine_runtime_policy_defaults_match_expected_values() {
    let resolved = HELIOS_ENGINE_TOKIO_POLICY.resolve();
    assert_eq!(resolved.worker_threads, 4);
    assert_eq!(resolved.max_blocking_threads, 4);
    assert_eq!(resolved.thread_stack_bytes, Some(2_097_152));
    assert_eq!(resolved.blocking_keep_alive.map(|value| value.as_millis()), Some(3_000));
}

#[test]
fn peripherals_runtime_policy_keeps_optional_stack_and_keepalive_unset() {
    let resolved = HELIOS_PERIPHERALS_TOKIO_POLICY.resolve();
    assert_eq!(resolved.worker_threads, 1);
    assert_eq!(resolved.max_blocking_threads, 1);
    assert_eq!(resolved.thread_stack_bytes, None);
    assert_eq!(resolved.blocking_keep_alive, None);
}

#[test]
fn api_startup_cache_warm_policy_defaults_match_expected_values() {
    let resolved = HELIOS_API_STARTUP_CACHE_WARM_POLICY.resolve();
    assert_eq!(resolved.initial_delay_ms, 1_500);
    assert_eq!(resolved.retry_delay_ms, 1_000);
    assert_eq!(resolved.attempts, 4);
}

#[test]
fn log_sources_policy_defaults_match_expected_values() {
    let resolved = HELIOS_API_LOG_SOURCES_POLICY.resolve();
    assert_eq!(resolved.cache_ms, 5_000);
    assert_eq!(resolved.refresh_timeout_ms, 3_000);
}

#[test]
fn i2c_inventory_policy_defaults_match_expected_values() {
    let resolved = HELIOS_I2C_INVENTORY_POLICY.resolve();
    assert_eq!(resolved.timeout_ms, 5_000);
    assert_eq!(resolved.cache_ttl_ms, 2_000);
}

#[test]
fn imu_runtime_policy_defaults_match_expected_values() {
    let resolved = HELIOS_IMU_RUNTIME_POLICY.resolve();
    assert_eq!(resolved.idle_interval_ms, 100);
}

#[test]
fn imu_fusion_policy_defaults_match_expected_values() {
    let resolved = HELIOS_IMU_FUSION_POLICY.resolve();
    assert_eq!(resolved.frame_correction_wxyz, None);
    assert_eq!(resolved.mag_norm_rel_tol, 0.45);
    assert_eq!(resolved.mag_norm_lp_tau_seconds, 6.0);
    assert_eq!(resolved.mag_min_horizontal, 0.15);
}

#[test]
fn ipc_journal_policy_defaults_match_expected_values() {
    let resolved = HELIOS_IPC_JOURNAL_POLICY.resolve();
    assert_eq!(resolved.dir, PathBuf::from("/var/lib/helios/journal/ipc"));
    assert_eq!(resolved.max_bytes, 8 * 1024 * 1024);
}

#[test]
fn dns_policy_defaults_match_expected_values() {
    let resolved = HELIOS_DNS_POLICY.resolve();
    assert_eq!(resolved.config_path, PathBuf::from("/etc/resolv.conf"));
}

#[test]
fn updater_filesystem_policy_defaults_match_expected_values() {
    let resolved = HELIOS_UPDATER_FILESYSTEM_POLICY.resolve();
    assert_eq!(resolved.socket_path, PathBuf::from("/run/helios/updater.sock"));
    assert_eq!(resolved.journal_path, PathBuf::from("/var/lib/helios/journal/updater.log"));
    assert_eq!(resolved.data_dir, PathBuf::from("/var/lib/helios"));
}

#[test]
fn peripherals_service_policy_defaults_match_expected_values() {
    let resolved = HELIOS_PERIPHERALS_SERVICE_POLICY.resolve();
    assert_eq!(resolved.socket_path, PathBuf::from("/run/helios/peripherals.sock"));
    assert_eq!(resolved.config_paths, None);
}

#[test]
fn engine_stream_runtime_policy_defaults_match_expected_values() {
    let resolved = HELIOS_ENGINE_STREAM_RUNTIME_POLICY.resolve();
    assert_eq!(resolved.host_buffer_default, 2);
    assert_eq!(resolved.host_buffer_max, 64);
    assert_eq!(resolved.preview_jpeg_quality_override, None);
}

#[test]
fn resource_guard_policy_defaults_match_expected_values() {
    let resolved = HELIOS_RESOURCE_GUARD_POLICY.resolve();
    assert!(resolved.enabled);
    assert_eq!(resolved.poll_ms, 1_500);
    assert_eq!(resolved.mem_low_kb, 700_000);
    assert_eq!(resolved.mem_recover_kb, 1_000_000);
    assert_eq!(resolved.cooldown_ms, 5_000);
    assert_eq!(resolved.metrics_top_n, 6);
    assert_eq!(resolved.metrics_timeout_ms, 300);
    assert!(!resolved.allow_stop_fallback);
    assert_eq!(resolved.stop_timeout_ms, 4_000);
}

#[test]
fn engine_crash_guard_policy_defaults_match_expected_values() {
    let resolved = HELIOS_ENGINE_CRASH_GUARD_POLICY.resolve();
    assert_eq!(resolved.window_ms, 60_000);
    assert_eq!(resolved.threshold, 3);
    assert_eq!(resolved.suppress_ms, 300_000);
    assert_eq!(resolved.min_downtime_ms, 2_000);
    assert_eq!(resolved.poll_ms, 1_000);
}

#[test]
fn api_streams_policy_defaults_match_expected_values() {
    let resolved = HELIOS_API_STREAMS_POLICY.resolve();
    assert_eq!(resolved.cache_ms, 750);
    assert_eq!(resolved.mjpeg_poll_ms, None);
    assert_eq!(resolved.snapshot_interval_ms, 33);
    assert_eq!(resolved.preview_max_fps, None);
    assert_eq!(resolved.preview_outage_ms, 15_000);
}

#[test]
fn api_localization_policy_defaults_match_expected_values() {
    let resolved = HELIOS_API_LOCALIZATION_POLICY.resolve();
    assert_eq!(resolved.media_seed_dir, PathBuf::from("/usr/share/helios/media"));
    assert_eq!(resolved.max_map_upload_bytes, 5 * 1024 * 1024);
    assert_eq!(resolved.solve_cache_entries, 64);
}

#[test]
fn api_hardware_read_model_policy_defaults_match_expected_values() {
    let resolved = HELIOS_API_HARDWARE_READ_MODEL_POLICY.resolve();
    assert_eq!(resolved.peripherals_cache_ms, 1_000);
    assert_eq!(resolved.peripherals_timeout_ms, 1_500);
    assert_eq!(resolved.camera_discovery_timeout_ms, 1_500);
    assert_eq!(resolved.peripherals_refresh_timeout_ms, 2_500);
}

#[test]
fn api_system_read_model_policy_defaults_match_expected_values() {
    let resolved = HELIOS_API_SYSTEM_READ_MODEL_POLICY.resolve();
    assert_eq!(resolved.sampler_thread_stack_bytes, 512 * 1024);
    assert_eq!(resolved.device_metrics_cache_ms, 750);
    assert_eq!(resolved.process_breakdown_limit, 16);
    assert_eq!(resolved.processes_sample_interval_ms, 1_000);
}

#[test]
fn peripherals_power_policy_defaults_match_expected_values() {
    let resolved = HELIOS_PERIPHERALS_POWER_POLICY.resolve();
    assert_eq!(resolved.poll_interval_ms, 100);
    assert_eq!(resolved.idle_interval_ms, 1_000);
}

#[test]
fn styx_capture_policy_is_optional_by_default() {
    let resolved = HELIOS_STYX_CAPTURE_TUNABLES_POLICY.resolve();
    assert_eq!(resolved.queue_depth, None);
    assert_eq!(resolved.pool_min, None);
    assert_eq!(resolved.pool_bytes, None);
    assert_eq!(resolved.pool_spare, None);
    assert!(!resolved.any_overridden());
}

#[test]
fn persistent_dir_policy_prefers_configured_env_override() {
    let _lock = env_lock();
    let dir = std::env::temp_dir().join(format!("helios-runtime-policy-env-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create temp data root");
    unsafe {
        std::env::set_var("HELIOS_API_DATA_DIR", &dir);
    }
    let resolved = HELIOS_API_DATA_ROOT_POLICY.resolve().expect("resolve data root");
    assert_eq!(resolved, dir);
    unsafe {
        std::env::remove_var("HELIOS_API_DATA_DIR");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn persistent_dir_policy_ignores_empty_env_and_uses_candidates() {
    let _lock = env_lock();
    let candidate = std::env::temp_dir().join(format!("helios-runtime-policy-candidate-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&candidate);
    let candidate_str: &'static str = Box::leak(candidate.to_string_lossy().into_owned().into_boxed_str());
    let candidates: &'static [&'static str] = Box::leak(Box::new([candidate_str]));
    unsafe {
        std::env::set_var("HELIOS_SHADOW_RECORD_DIR", "   ");
    }
    let resolved = PersistentDirPolicy { env_vars: &["HELIOS_SHADOW_RECORD_DIR"], candidates }.resolve().expect("resolve fallback candidate");
    assert_eq!(resolved, PathBuf::from(&candidate));
    unsafe {
        std::env::remove_var("HELIOS_SHADOW_RECORD_DIR");
    }
    assert!(candidate.is_dir());
    let shadow_policy = HELIOS_SHADOW_RECORD_DATA_ROOT_POLICY;
    assert!(!shadow_policy.candidates.is_empty());
    let _ = std::fs::remove_dir_all(&candidate);
}

#[test]
fn daedalus_runtime_policy_defaults_match_expected_values() {
    let resolved = HELIOS_DAEDALUS_RUNTIME_POLICY.resolve();
    assert_eq!(resolved.install_dir, PathBuf::from("/var/lib/helios/plugins/daedalus"));
    assert_eq!(resolved.upload_dir, PathBuf::from("/var/lib/helios/plugins/uploads"));
    assert_eq!(resolved.registry_snapshot_path, PathBuf::from("/var/lib/helios/state/node-registry.snapshot.json"));
    assert_eq!(resolved.registry_generator_binary, PathBuf::from("/usr/bin/helios-engine"));
    assert_eq!(resolved.plugin_search_dirs, vec![PathBuf::from("/var/lib/helios/plugins/daedalus"), PathBuf::from("/usr/lib/helios/plugins/daedalus")]);
    assert_eq!(resolved.max_plugin_upload_bytes, 64 * 1024 * 1024);
}

#[test]
fn daedalus_runtime_policy_prefers_single_plugin_dir_override() {
    let _lock = env_lock();
    unsafe {
        std::env::set_var("HELIOS_DAEDALUS_PLUGIN_DIR", "/tmp/helios-plugin-override");
        std::env::remove_var("HELIOS_DAEDALUS_PLUGIN_DIRS");
    }
    let resolved = HELIOS_DAEDALUS_RUNTIME_POLICY.resolve();
    assert_eq!(resolved.plugin_search_dirs, vec![PathBuf::from("/tmp/helios-plugin-override")]);
    unsafe {
        std::env::remove_var("HELIOS_DAEDALUS_PLUGIN_DIR");
    }
}

#[test]
fn engine_ipc_policy_defaults_match_expected_values() {
    let resolved = HELIOS_ENGINE_IPC_POLICY.resolve();
    assert_eq!(resolved.socket, PathBuf::from("/run/helios/engine.sock"));
    assert_eq!(resolved.event_buffer, 4096);
    assert_eq!(resolved.metrics_broadcast_interval.as_millis(), 5_000);
    assert_eq!(resolved.calibration_solve_timeout.as_secs(), 300);
    assert_eq!(resolved.localization_solve_timeout.as_millis(), 30_000);
    assert_eq!(resolved.request_send_timeout.as_millis(), 1_000);
    assert_eq!(resolved.command_send_timeout.as_millis(), 2_000);
    assert_eq!(resolved.request_queue_base, 256);
    assert_eq!(resolved.request_queue_per_stream, 8);
    assert_eq!(resolved.request_queue_max, 2_048);
    assert_eq!(resolved.timeout_scale_for_streams(4), 1_200_000);
    assert_eq!(resolved.request_queue_size(4), 288);
}

#[test]
fn engine_graph_policy_defaults_match_expected_values() {
    let resolved = HELIOS_ENGINE_GRAPH_POLICY.resolve();
    assert_eq!(resolved.pool_size, None);
    assert_eq!(resolved.runtime_queue_cap, 4);
    assert!(!resolved.dedicated_executor);
    assert_eq!(resolved.executor_busy, EngineExecutorBusyPolicy::Drop);
    assert_eq!(resolved.executor_busy_timeout_ms, None);
    assert!(resolved.auto_target_roi);
    assert!(!resolved.host_outputs_in_graph);
    assert!(!resolved.demand_driven);
    assert!(!resolved.host_output_debug);
    assert!(!resolved.perf_counters);
    assert!(!resolved.pprof_enabled);
    assert_eq!(resolved.background_trim_interval_ms, 5_000);
    assert_eq!(resolved.active_trim_interval_ms, 0);
    assert_eq!(resolved.host_output_sample_ttl_ms, 500);
    assert_eq!(resolved.pprof_frames, 1);
    assert_eq!(resolved.pprof_duration_ms, None);
}

#[test]
fn engine_recording_policy_defaults_match_expected_values() {
    let resolved = HELIOS_ENGINE_RECORDING_POLICY.resolve();
    assert_eq!(resolved.stream_command_queue_size, 64);
    assert_eq!(resolved.recording_frame_queue_size, 48);
    assert_eq!(resolved.stream_worker_stack_bytes, 2 * 1024 * 1024);
    assert_eq!(resolved.recording_worker_stack_bytes, 1024 * 1024);
    assert_eq!(resolved.recording_stop_grace_ms, 0);
    assert_eq!(resolved.shadow_window_ms, 120_000);
    assert_eq!(resolved.shadow_segment_ms, 2_000);
    assert_eq!(resolved.shadow_flush_interval_ms, 1_000);
    assert_eq!(resolved.shadow_writer_buffer_bytes, 1 << 20);
    assert_eq!(resolved.shadow_config_scan_interval_ms, 1_000);
    assert!(!resolved.keep_raw_on_record_fail);
    assert!(resolved.shadow_recorder_enabled);
    assert!(!resolved.recording_encoded_passthrough);
    assert!(!resolved.recording_shadow_start_stop);
    assert!(!resolved.rewrite_encoded_frame_timestamps_to_wall);
}

#[test]
fn log_filter_policy_defaults_to_info() {
    assert_eq!(HELIOS_LOG_FILTER_POLICY.default, "info");
}

#[test]
fn classify_platform_family_detects_raspberry_pi() {
    assert_eq!(classify_platform_family(Some("Raspberry Pi Compute Module 5 Rev 1.0"), "aarch64"), PlatformFamily::RaspberryPi);
}

#[test]
fn classify_platform_family_falls_back_to_generic_linux_for_aarch64() {
    assert_eq!(classify_platform_family(None, "aarch64"), PlatformFamily::GenericLinux);
}
