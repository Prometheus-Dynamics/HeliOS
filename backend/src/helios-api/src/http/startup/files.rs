use std::ffi::OsStr;
use std::io;
use std::path::{Path, PathBuf};
use tokio::fs;

use super::{StartupPresetDocument, StartupPresetMarker};
use crate::http::storage;

const DEFAULT_STARTUP_PRESET_TOML_PATH: &str = "/etc/helios/startup.toml";
const LEGACY_STARTUP_PRESET_JSON_PATH: &str = "/etc/helios/startup.json";
const STARTUP_PRESET_FILE_ENV: &str = "HELIOS_STARTUP_PRESET_FILE";
const STARTUP_PRESET_MARKER_ENV: &str = "HELIOS_STARTUP_PRESET_MARKER";
const STARTUP_PRESET_MARKER_NAME: &str = ".startup-preset-applied-v1.json";

pub(super) fn startup_preset_path() -> PathBuf {
    if let Some(path) = std::env::var_os(STARTUP_PRESET_FILE_ENV).map(PathBuf::from) {
        return path;
    }
    let toml_default = PathBuf::from(DEFAULT_STARTUP_PRESET_TOML_PATH);
    if toml_default.exists() { toml_default } else { PathBuf::from(LEGACY_STARTUP_PRESET_JSON_PATH) }
}

pub(super) fn startup_marker_path() -> PathBuf {
    std::env::var_os(STARTUP_PRESET_MARKER_ENV).map(PathBuf::from).unwrap_or_else(|| storage::data_root_path().join(STARTUP_PRESET_MARKER_NAME))
}

pub(super) async fn write_marker(path: &Path, marker: StartupPresetMarker) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).await?;
    }
    let bytes = serde_json::to_vec_pretty(&marker).map_err(io::Error::other)?;
    fs::write(path, bytes).await
}

pub(super) fn decode_startup_preset(path: &Path, bytes: &[u8]) -> Result<StartupPresetDocument, String> {
    let ext = path.extension().and_then(OsStr::to_str).map(|value| value.to_ascii_lowercase());

    match ext.as_deref() {
        Some("toml") => toml::from_str::<StartupPresetDocument>(&String::from_utf8_lossy(bytes))
            .map_err(|err| err.to_string())
            .or_else(|toml_err| serde_json::from_slice::<StartupPresetDocument>(bytes).map_err(|json_err| format!("toml parse error: {toml_err}; json parse error: {json_err}"))),
        Some("json") => serde_json::from_slice::<StartupPresetDocument>(bytes)
            .map_err(|err| err.to_string())
            .or_else(|json_err| toml::from_str::<StartupPresetDocument>(&String::from_utf8_lossy(bytes)).map_err(|toml_err| format!("json parse error: {json_err}; toml parse error: {toml_err}"))),
        _ => serde_json::from_slice::<StartupPresetDocument>(bytes).or_else(|_| toml::from_str::<StartupPresetDocument>(&String::from_utf8_lossy(bytes))).map_err(|err| err.to_string()),
    }
}

pub(super) async fn pipeline_document_count() -> io::Result<usize> {
    let dir = storage::ensure_subdir_async("pipelines").await?;
    let mut entries = fs::read_dir(dir).await?;
    let mut count = 0usize;
    while let Some(entry) = entries.next_entry().await? {
        if !entry.file_type().await.map(|ty| ty.is_file()).unwrap_or(false) {
            continue;
        }
        if entry.path().extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        count += 1;
    }
    Ok(count)
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::decode_startup_preset;

    #[test]
    fn startup_toml_decodes() {
        let raw = r#"
            [[pipelines]]
            id = "4fca14eb-e218-48e4-adf9-b957d645c604"
            name = "Default ArUco"
            templateId = "daedalus_aruco"

            [[streams]]
            cameraId = "ov9782"
            streamId = "43e2cb68-95b0-4269-97da-7e16c730c36a"

            [streams.manifest.identity]
            id = "43e2cb68-95b0-4269-97da-7e16c730c36a"
            alias = "ov9782-csi"
            hardware_id = "ov9782"

            [streams.manifest.capture]
            backend = "Libcamera"
            target_fps = 120
            enable_tdn_output = true
            device_keys = ["ov9782"]

            [streams.manifest.capture.handle]
            type = "libcamera"
            id = "ov9782"

            [streams.manifest.capture.mode]
            interval = { numerator = 1, denominator = 120 }

            [streams.manifest.capture.mode.format]
            code = "NV12"
            color = "Unknown"
            resolution = { width = 1280, height = 720 }

            [[streams.manifest.capture.controls]]
            id = 10002
            value = { kind = "int", value = 3 }

            [[streams.manifest.pipelines]]
            pipeline_id = "4fca14eb-e218-48e4-adf9-b957d645c604"
            pipeline_output = "overlay"

            [streams.manifest.pipeline_layout]
            rows = 1
            columns = 1

            [[streams.manifest.pipeline_layout.slots]]
            row = 0
            column = 0
            pipeline_id = "4fca14eb-e218-48e4-adf9-b957d645c604"
            output_key = "overlay"

            [streams.manifest]
            pipeline_enabled = true
            active_pipeline_id = "4fca14eb-e218-48e4-adf9-b957d645c604"
            active_pipeline_output = "overlay"
            start_on_boot = true

            [streams.manifest.encoder]
            state = "enabled"

            [streams.manifest.encoder.settings]
            framerate = { numerator = 20, denominator = 1 }
            output_resolution = { width = 854, height = 480 }

            [streams.manifest.decoder]
            state = "enabled"
            id = "nv12-luma"

            [streams.manifest.decoder.settings]
            fps_limit = 24
            rotation_degrees = 90
            mirror_horizontal = true
        "#;

        let doc = decode_startup_preset(Path::new("/etc/helios/startup.toml"), raw.as_bytes()).expect("decode startup preset");
        assert_eq!(doc.pipelines.len(), 1);
        assert_eq!(doc.streams.len(), 1);
        assert_eq!(doc.streams[0].camera_id, "ov9782");
        assert_eq!(doc.streams[0].manifest.capture.target_fps, Some(120));
        assert_eq!(doc.streams[0].manifest.encoder.settings().and_then(|enc| enc.output_resolution.as_ref()).map(|res| (res.width, res.height)), Some((854, 480)));
        assert_eq!(doc.streams[0].manifest.decoder.id(), Some("nv12-luma"));
        assert_eq!(doc.streams[0].manifest.decoder.settings().and_then(|decoder| decoder.rotation_degrees), Some(90));
    }
}
