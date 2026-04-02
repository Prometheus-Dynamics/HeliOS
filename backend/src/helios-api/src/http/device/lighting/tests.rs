use helios_peripherals::dto::{LightingAnimation, LightingColor, LightingCommand};
use lib_led_animations::{LedAnimationEntry, LedAnimationTimeline};

use super::{
    animations::entry_to_response,
    templates::{LightingAnimationTemplateDocumentRaw, normalize_template_id, template_raw_into_document},
    types::{LightingAnimationPayload, LightingColorPayload, LightingFramePayload, LightingTimelineEasingPayload, LightingTimelineKeyframePayload, LightingTimelinePayload},
};

#[test]
fn normalize_template_id_rejects_empty_and_sanitizes_json_suffix() {
    assert_eq!(normalize_template_id(" demo.json "), Some("demo".to_string()));
    assert_eq!(normalize_template_id("nested/path/name.json"), Some("name".to_string()));
    assert_eq!(normalize_template_id("   "), None);
}

#[test]
fn template_raw_into_document_materializes_sequence_from_timeline() {
    let raw = LightingAnimationTemplateDocumentRaw {
        schema_version: 1,
        name: Some("Pulse".to_string()),
        timeline: Some(LightingTimelinePayload {
            duration_ms: Some(200),
            sample_ms: Some(100),
            keyframes: vec![
                LightingTimelineKeyframePayload { time_ms: 0, frame: vec![LightingColorPayload { r: 255, g: 0, b: 0, w: 0 }], easing: LightingTimelineEasingPayload::Linear },
                LightingTimelineKeyframePayload { time_ms: 200, frame: vec![LightingColorPayload { r: 0, g: 0, b: 255, w: 0 }], easing: LightingTimelineEasingPayload::Linear },
            ],
        }),
        ..Default::default()
    };

    let doc = template_raw_into_document("pulse".to_string(), raw).expect("template should be valid");
    assert_eq!(doc.id, "pulse");
    assert_eq!(doc.name, "Pulse");
    assert!(doc.timeline.is_some());
    assert!(doc.frames.as_ref().is_some_and(|frames| !frames.is_empty()));
}

#[test]
fn entry_to_response_preserves_sequence_and_animation() {
    let entry = LedAnimationEntry {
        name: "demo".to_string(),
        command: LightingCommand { frame: None, brightness: Some(64), animation: Some(LightingAnimation::Chase { color: LightingColor { r: 10, g: 20, b: 30, w: 0 }, speed_hz: 2.0 }) },
        duration_ms: Some(250),
        sequence: vec![lib_led_animations::LedAnimationFrame { frame: vec![LightingColor { r: 5, g: 6, b: 7, w: 0 }], duration_ms: 50 }],
        timeline: Some(LedAnimationTimeline { duration_ms: Some(250), sample_ms: Some(50), keyframes: vec![] }),
    };

    let response = entry_to_response(entry);
    assert_eq!(response.name, "demo");
    assert_eq!(response.brightness, Some(64));
    assert!(matches!(response.animation, Some(LightingAnimationPayload::Chase { speed_hz, .. }) if (speed_hz - 2.0).abs() < f32::EPSILON));
    assert_eq!(response.frames, Some(vec![LightingFramePayload { frame: vec![LightingColorPayload { r: 5, g: 6, b: 7, w: 0 }], duration_ms: 50 }]));
}

#[test]
fn lighting_template_decode_rejects_missing_schema_version() {
    let raw = serde_json::json!({
        "name": "Pulse",
        "frame": [{ "r": 255, "g": 0, "b": 0, "w": 0 }]
    });

    let err = LightingAnimationTemplateDocumentRaw::decode_str(&serde_json::to_string(&raw).expect("encode")).expect_err("missing schema version should fail");
    assert!(err.contains("missing required schema_version"));
}

#[test]
fn lighting_template_decode_rejects_future_schema_version() {
    let raw = serde_json::json!({
        "schema_version": 2,
        "frame": [{ "r": 255, "g": 0, "b": 0, "w": 0 }]
    });

    let err = LightingAnimationTemplateDocumentRaw::decode_str(&serde_json::to_string(&raw).expect("encode")).expect_err("future schema should fail");
    assert!(err.contains("unsupported lighting template document schema_version"));
}

#[test]
fn lighting_template_decode_rejects_legacy_command_shape() {
    let raw = serde_json::json!({
        "schema_version": 1,
        "command": {
            "frame": [{ "r": 1, "g": 2, "b": 3, "w": 4 }],
            "brightness": 32
        }
    });

    let err = LightingAnimationTemplateDocumentRaw::decode_str(&serde_json::to_string(&raw).expect("encode")).expect_err("legacy nested command payload should fail");
    assert!(err.contains("unknown field `command`"));
}
