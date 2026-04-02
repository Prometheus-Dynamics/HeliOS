use lib_schema_migration::{SyncSchemaPlan, migrate_to_current};
use std::ffi::OsStr;
use std::io;
use std::path::{Path, PathBuf};
use tokio::fs;

use super::{CURRENT_STARTUP_PRESET_SCHEMA_VERSION, StartupPresetDocument, StartupPresetMarker};
use crate::http::storage;

const DEFAULT_STARTUP_PRESET_TOML_PATH: &str = "/var/lib/helios/startup.toml";
const STARTUP_PRESET_FILE_ENV: &str = "HELIOS_STARTUP_PRESET_FILE";
const STARTUP_PRESET_MARKER_ENV: &str = "HELIOS_STARTUP_PRESET_MARKER";
const STARTUP_PRESET_MARKER_NAME: &str = ".startup-preset-applied-v1.json";

fn startup_preset_path() -> PathBuf {
    match std::env::var_os(STARTUP_PRESET_FILE_ENV) {
        Some(path) => PathBuf::from(path),
        None => PathBuf::from(DEFAULT_STARTUP_PRESET_TOML_PATH),
    }
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

fn decode_startup_preset_with_dirty(path: &Path, bytes: &[u8]) -> Result<(StartupPresetDocument, bool), String> {
    let ext = path.extension().and_then(OsStr::to_str).map(|value| value.to_ascii_lowercase());

    match ext.as_deref() {
        Some("toml") => decode_startup_preset_toml(&String::from_utf8_lossy(bytes))
            .or_else(|toml_err| decode_startup_preset_json(bytes).map_err(|json_err| format!("toml parse error: {toml_err}; json parse error: {json_err}"))),
        Some("json") => decode_startup_preset_json(bytes)
            .or_else(|json_err| decode_startup_preset_toml(&String::from_utf8_lossy(bytes)).map_err(|toml_err| format!("json parse error: {json_err}; toml parse error: {toml_err}"))),
        _ => decode_startup_preset_json(bytes).or_else(|_| decode_startup_preset_toml(&String::from_utf8_lossy(bytes))),
    }
}

async fn write_canonical_startup_preset(path: &Path, preset: &StartupPresetDocument) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).await?;
    }
    let mut canonical = preset.clone();
    canonical.schema_version = CURRENT_STARTUP_PRESET_SCHEMA_VERSION;
    let encoded = toml::to_string_pretty(&canonical).map_err(io::Error::other)?;
    fs::write(path, encoded).await
}

const STARTUP_PRESET_JSON_SCHEMA_PLAN: SyncSchemaPlan<serde_json::Value> =
    SyncSchemaPlan { document_name: "startup preset document", legacy_version: CURRENT_STARTUP_PRESET_SCHEMA_VERSION, current_version: CURRENT_STARTUP_PRESET_SCHEMA_VERSION, migrations: &[] };

const STARTUP_PRESET_TOML_SCHEMA_PLAN: SyncSchemaPlan<toml::Value> =
    SyncSchemaPlan { document_name: "startup preset document", legacy_version: CURRENT_STARTUP_PRESET_SCHEMA_VERSION, current_version: CURRENT_STARTUP_PRESET_SCHEMA_VERSION, migrations: &[] };

fn decode_startup_preset_json(bytes: &[u8]) -> Result<(StartupPresetDocument, bool), String> {
    let raw = serde_json::from_slice::<serde_json::Value>(bytes).map_err(|err| err.to_string())?;
    let migrated = migrate_to_current(raw.clone(), &STARTUP_PRESET_JSON_SCHEMA_PLAN)?;
    let parsed = serde_json::from_value(migrated.clone()).map_err(|err| err.to_string())?;
    Ok((parsed, migrated != raw))
}

fn decode_startup_preset_toml(raw: &str) -> Result<(StartupPresetDocument, bool), String> {
    let value = toml::from_str::<toml::Value>(raw).map_err(|err| err.to_string())?;
    let migrated = migrate_to_current(value.clone(), &STARTUP_PRESET_TOML_SCHEMA_PLAN)?;
    let parsed = migrated.clone().try_into().map_err(|err: toml::de::Error| err.to_string())?;
    Ok((parsed, migrated != value))
}

pub(super) async fn load_startup_preset() -> io::Result<Option<(PathBuf, StartupPresetDocument)>> {
    let configured_path = startup_preset_path();
    load_startup_preset_from_path(&configured_path).await
}

async fn load_startup_preset_from_path(configured_path: &Path) -> io::Result<Option<(PathBuf, StartupPresetDocument)>> {
    match fs::read(configured_path).await {
        Ok(bytes) => {
            let (preset, dirty) = decode_startup_preset_with_dirty(configured_path, &bytes).map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
            if dirty {
                write_canonical_startup_preset(configured_path, &preset).await?;
            }
            return Ok(Some((configured_path.to_path_buf(), preset)));
        }
        Err(err) if err.kind() == io::ErrorKind::NotFound => {}
        Err(err) => return Err(err),
    }
    Ok(None)
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

    use tempfile::{Builder, TempDir, tempdir};

    use super::{decode_startup_preset_with_dirty, load_startup_preset_from_path};
    use crate::http::startup::CURRENT_STARTUP_PRESET_SCHEMA_VERSION;

    fn startup_tempdir() -> TempDir {
        if Path::new("/dev/shm").is_dir() { Builder::new().prefix("helios-startup-files-").tempdir_in("/dev/shm").expect("tempdir in /dev/shm") } else { tempdir().expect("tempdir") }
    }

    #[test]
    fn startup_toml_decodes() {
        let raw = r#"
            schema_version = 1

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
            schema_version = 1
            pipeline_enabled = true
            active_pipeline_id = "4fca14eb-e218-48e4-adf9-b957d645c604"
            active_pipeline_output = "overlay"
            start_on_boot = true

            [streams.manifest.encoder]
            state = "enabled"
            id = "h264"

            [streams.manifest.encoder.settings]
            kind = "h264"
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

        let (doc, _) = decode_startup_preset_with_dirty(Path::new("/var/lib/helios/startup.toml"), raw.as_bytes()).expect("decode startup preset");
        assert_eq!(doc.schema_version, CURRENT_STARTUP_PRESET_SCHEMA_VERSION);
        assert_eq!(doc.pipelines.len(), 1);
        assert_eq!(doc.streams.len(), 1);
        assert_eq!(doc.streams[0].camera_id, "ov9782");
        assert_eq!(doc.streams[0].manifest.capture.target_fps, Some(120));
        assert_eq!(doc.streams[0].manifest.encoder.settings().and_then(|enc| enc.output_resolution()).map(|res| (res.width, res.height)), Some((854, 480)));
        assert_eq!(doc.streams[0].manifest.decoder.id(), Some("nv12-luma"));
        assert_eq!(doc.streams[0].manifest.decoder.settings().and_then(|decoder| decoder.rotation_degrees), Some(90));
    }

    #[test]
    fn startup_json_decodes() {
        let raw = r#"{
          "schema_version": 1,
          "pipelines": [
            {
              "id": "4fca14eb-e218-48e4-adf9-b957d645c604",
              "name": "Default ArUco",
              "templateId": "daedalus_aruco"
            }
          ],
          "streams": [
            {
              "cameraId": "ov9782",
              "streamId": "43e2cb68-95b0-4269-97da-7e16c730c36a",
              "manifest": {
                "schema_version": 1,
                "identity": {
                  "id": "43e2cb68-95b0-4269-97da-7e16c730c36a",
                  "alias": "ov9782-csi",
                  "hardware_id": "ov9782"
                },
                "capture": {
                  "backend": "Libcamera",
                  "target_fps": 120,
                  "enable_tdn_output": true,
                  "device_keys": ["ov9782"],
                  "handle": {
                    "type": "libcamera",
                    "id": "ov9782"
                  },
                  "mode": {
                    "interval": { "numerator": 1, "denominator": 120 },
                    "format": {
                      "code": "NV12",
                      "color": "Unknown",
                      "resolution": { "width": 1280, "height": 720 }
                    }
                  },
                  "controls": [
                    { "id": 10002, "value": { "kind": "int", "value": 3 } }
                  ]
                },
                "pipelines": [
                  { "pipeline_id": "4fca14eb-e218-48e4-adf9-b957d645c604", "pipeline_output": "overlay" }
                ],
                "pipeline_layout": {
                  "rows": 1,
                  "columns": 1,
                  "slots": [
                    {
                      "row": 0,
                      "column": 0,
                      "pipeline_id": "4fca14eb-e218-48e4-adf9-b957d645c604",
                      "output_key": "overlay"
                    }
                  ]
                },
                "pipeline_enabled": true,
                "active_pipeline_id": "4fca14eb-e218-48e4-adf9-b957d645c604",
                "active_pipeline_output": "overlay",
                "start_on_boot": true,
                "encoder": {
                  "state": "enabled",
                  "id": "h264",
                  "settings": {
                    "kind": "h264",
                    "framerate": { "numerator": 20, "denominator": 1 },
                    "output_resolution": { "width": 854, "height": 480 }
                  }
                },
                "decoder": {
                  "state": "enabled",
                  "id": "nv12-luma",
                  "settings": {
                    "fps_limit": 24,
                    "rotation_degrees": 90,
                    "mirror_horizontal": true
                  }
                }
              }
            }
          ]
        }"#;

        let (doc, _) = decode_startup_preset_with_dirty(Path::new("/var/lib/helios/startup.json"), raw.as_bytes()).expect("decode startup preset");
        assert_eq!(doc.schema_version, CURRENT_STARTUP_PRESET_SCHEMA_VERSION);
        assert_eq!(doc.pipelines.len(), 1);
        assert_eq!(doc.streams.len(), 1);
        assert_eq!(doc.streams[0].camera_id, "ov9782");
        assert_eq!(doc.streams[0].manifest.capture.target_fps, Some(120));
        assert_eq!(doc.streams[0].manifest.encoder.settings().and_then(|enc| enc.output_resolution()).map(|res| (res.width, res.height)), Some((854, 480)));
        assert_eq!(doc.streams[0].manifest.decoder.id(), Some("nv12-luma"));
        assert_eq!(doc.streams[0].manifest.decoder.settings().and_then(|decoder| decoder.rotation_degrees), Some(90));
    }

    #[test]
    fn startup_preset_rejects_future_schema_version() {
        let raw = format!("schema_version = {}\npipelines = []\nstreams = []\n", CURRENT_STARTUP_PRESET_SCHEMA_VERSION + 1);
        let err = decode_startup_preset_with_dirty(Path::new("/var/lib/helios/startup.toml"), raw.as_bytes()).expect_err("future schema should fail");
        assert!(err.contains("unsupported startup preset document schema_version"));
    }

    #[tokio::test]
    async fn load_startup_preset_reads_canonical_toml() {
        let temp = startup_tempdir();
        let canonical = temp.path().join("startup.toml");
        tokio::fs::write(
            &canonical,
            r#"
                schema_version = 1
                pipelines = []
                streams = []
            "#,
        )
        .await
        .expect("write canonical toml");

        let loaded = load_startup_preset_from_path(&canonical).await.expect("load startup preset").expect("startup preset present");
        assert_eq!(loaded.0, canonical);
        assert_eq!(loaded.1.schema_version, CURRENT_STARTUP_PRESET_SCHEMA_VERSION);
        let canonical_raw = tokio::fs::read_to_string(&canonical).await.expect("read canonical");
        let (decoded, _) = decode_startup_preset_with_dirty(&canonical, canonical_raw.as_bytes()).expect("decode canonical startup preset");
        assert_eq!(decoded.schema_version, CURRENT_STARTUP_PRESET_SCHEMA_VERSION);
    }

    #[test]
    fn startup_preset_rejects_missing_schema_version() {
        let raw = "pipelines = []\nstreams = []\n";
        let err = decode_startup_preset_with_dirty(Path::new("/var/lib/helios/startup.toml"), raw.as_bytes()).expect_err("missing schema version should fail");
        assert!(err.contains("missing required schema_version"));
    }
}
