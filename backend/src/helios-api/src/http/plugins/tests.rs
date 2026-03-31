use std::path::PathBuf;

use crate::http::error::ApiError;

use super::support::{DISABLED_SUFFIX, PLUGIN_SUFFIX, build_compatibility_map, ensure_plugin_filename, guess_content_type, max_upload_bytes, plugin_dirs};

#[test]
fn ensure_plugin_filename_requires_shared_object_suffix() {
    assert!(ensure_plugin_filename("plugin.so").is_ok());
    let err: Box<ApiError> = ensure_plugin_filename("plugin.txt").expect_err("expected error");
    assert!(err.to_string().contains(".so"));
}

#[test]
fn plugin_dirs_prefers_single_dir_override() {
    unsafe {
        std::env::set_var("HELIOS_DAEDALUS_PLUGIN_DIR", "/tmp/custom-plugin-dir");
    }
    unsafe {
        std::env::remove_var("HELIOS_DAEDALUS_PLUGIN_DIRS");
    }

    let dirs = plugin_dirs();
    assert_eq!(dirs, vec![PathBuf::from("/tmp/custom-plugin-dir")]);

    unsafe {
        std::env::remove_var("HELIOS_DAEDALUS_PLUGIN_DIR");
    }
}

#[test]
fn plugin_dirs_uses_split_paths_override() {
    unsafe {
        std::env::remove_var("HELIOS_DAEDALUS_PLUGIN_DIR");
    }
    let joined = std::env::join_paths([PathBuf::from("/tmp/plugin-a"), PathBuf::from("/tmp/plugin-b")]).expect("join paths");
    unsafe {
        std::env::set_var("HELIOS_DAEDALUS_PLUGIN_DIRS", &joined);
    }

    let dirs = plugin_dirs();
    assert_eq!(dirs, vec![PathBuf::from("/tmp/plugin-a"), PathBuf::from("/tmp/plugin-b")]);

    unsafe {
        std::env::remove_var("HELIOS_DAEDALUS_PLUGIN_DIRS");
    }
}

#[test]
fn max_upload_bytes_accepts_positive_mb_override() {
    unsafe {
        std::env::set_var("HELIOS_API_MAX_PLUGIN_MB", "12");
    }
    assert_eq!(max_upload_bytes(), 12 * 1024 * 1024);
    unsafe {
        std::env::remove_var("HELIOS_API_MAX_PLUGIN_MB");
    }
}

#[test]
fn guess_content_type_defaults_to_octet_stream_for_plugins() {
    assert_eq!(guess_content_type("plugin.so"), "application/octet-stream");
}

#[test]
fn build_compatibility_map_preserves_filename_keys() {
    let entries = vec![
        helios_engine::ipc::PluginCompatibility {
            filename: format!("example{PLUGIN_SUFFIX}"),
            plugin_name: Some("Example".into()),
            plugin_version: Some("1.0".into()),
            status: "ok".into(),
            reason: None,
            expected_daedalus_version: "x".into(),
            daedalus_version: Some("x".into()),
            expected_ffi_version: "y".into(),
            ffi_version: Some("y".into()),
            expected_abi_version: 1,
            abi_version: Some(1),
            path: "/tmp/example.so".into(),
        },
        helios_engine::ipc::PluginCompatibility {
            filename: format!("disabled{DISABLED_SUFFIX}"),
            plugin_name: Some("Disabled".into()),
            plugin_version: Some("2.0".into()),
            status: "disabled".into(),
            reason: Some("plugin disabled".into()),
            expected_daedalus_version: "x".into(),
            daedalus_version: Some("x".into()),
            expected_ffi_version: "y".into(),
            ffi_version: Some("y".into()),
            expected_abi_version: 1,
            abi_version: Some(1),
            path: "/tmp/disabled.so.disabled".into(),
        },
    ];

    let map = build_compatibility_map(entries);
    assert!(map.contains_key("example.so"));
    assert!(map.contains_key("disabled.so.disabled"));
}
