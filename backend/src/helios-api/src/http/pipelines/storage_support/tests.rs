use super::{PipelineTemplateDocumentRaw, template_raw_into_summary};

#[test]
fn template_raw_into_summary_uses_template_id_when_name_is_blank() {
    let raw = PipelineTemplateDocumentRaw {
        schema_version: 1,
        id: Some("demo".to_string()),
        name: Some("   ".to_string()),
        summary: Some("  Example  ".to_string()),
        tags: vec!["vision".to_string()],
        graph: serde_json::json!({ "nodes": [], "edges": [] }),
    };

    let summary = template_raw_into_summary("demo".to_string(), raw);
    assert_eq!(summary.name, "demo");
    assert_eq!(summary.summary.as_deref(), Some("Example"));
}
