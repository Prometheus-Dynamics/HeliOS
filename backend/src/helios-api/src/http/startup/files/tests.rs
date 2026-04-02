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
