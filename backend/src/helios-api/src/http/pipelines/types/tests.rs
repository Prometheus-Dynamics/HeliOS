use super::documents::{CURRENT_PIPELINE_DOCUMENT_SCHEMA_VERSION, CURRENT_PIPELINE_TEMPLATE_DOCUMENT_SCHEMA_VERSION};
use super::{PipelineDocument, PipelineTemplateDocumentRaw};
use uuid::Uuid;

#[test]
fn pipeline_document_decode_rejects_missing_schema_version() {
    let pipeline_id = Uuid::new_v4();
    let raw = serde_json::json!({
        "id": pipeline_id,
        "name": "demo",
        "graph": { "nodes": [], "edges": [] },
        "updated_at_ms": 123
    });

    let err = PipelineDocument::decode_slice(serde_json::to_string(&raw).expect("encode").as_bytes()).expect_err("missing schema version should fail");
    assert!(err.contains("missing required schema_version"));
}

#[test]
fn pipeline_document_decode_rejects_future_schema_version() {
    let raw = serde_json::json!({
        "schema_version": CURRENT_PIPELINE_DOCUMENT_SCHEMA_VERSION + 1,
        "id": Uuid::new_v4(),
        "graph": { "nodes": [], "edges": [] },
        "updated_at_ms": 123
    });

    let err = PipelineDocument::decode_slice(serde_json::to_string(&raw).expect("encode").as_bytes()).expect_err("future version should fail");
    assert!(err.contains("unsupported pipeline document schema_version"));
}

#[test]
fn pipeline_document_encode_stamps_current_schema_version() {
    let encoded =
        PipelineDocument { schema_version: 0, id: Uuid::new_v4(), name: None, graph: serde_json::json!({"nodes":[],"edges":[]}), updated_at_ms: 55 }.encode_pretty().expect("encode pipeline");
    let parsed: serde_json::Value = serde_json::from_slice(&encoded).expect("decode json");
    assert_eq!(parsed.get("schema_version").and_then(serde_json::Value::as_u64), Some(CURRENT_PIPELINE_DOCUMENT_SCHEMA_VERSION as u64));
}

#[test]
fn pipeline_template_document_decode_accepts_builtin_template_without_schema_version() {
    let raw = serde_json::json!({
        "id": "demo",
        "name": "Demo Template",
        "graph": { "nodes": [], "edges": [] }
    });

    let parsed = PipelineTemplateDocumentRaw::decode_str(&serde_json::to_string(&raw).expect("encode")).expect("builtin template without schema_version should decode");
    assert_eq!(parsed.schema_version, CURRENT_PIPELINE_TEMPLATE_DOCUMENT_SCHEMA_VERSION);
    assert_eq!(parsed.id.as_deref(), Some("demo"));
}

#[test]
fn pipeline_template_document_decode_rejects_future_schema_version() {
    let raw = serde_json::json!({
        "schema_version": CURRENT_PIPELINE_TEMPLATE_DOCUMENT_SCHEMA_VERSION + 1,
        "id": "demo",
        "graph": { "nodes": [], "edges": [] }
    });

    let err = PipelineTemplateDocumentRaw::decode_str(&serde_json::to_string(&raw).expect("encode")).expect_err("future version should fail");
    assert!(err.contains("unsupported pipeline template document schema_version"));
}
