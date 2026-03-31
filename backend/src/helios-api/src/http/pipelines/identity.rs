use axum::{Json, http::StatusCode, response::IntoResponse};
use std::collections::BTreeSet;
use tokio::fs;
use tracing::warn;
use uuid::Uuid;

use crate::http::identity_tokens;

use super::{
    graph_support::pipeline_graph_alias,
    storage_support::map_io_error,
    types::{PipelineDocument, PipelineError},
};

#[derive(Debug, Clone)]
pub(super) struct PipelineIdentityConflict {
    pub token: String,
    pub existing_id: Option<Uuid>,
    pub existing_name: Option<String>,
    pub existing_alias: Option<String>,
}

pub(super) fn pipeline_identity_token_set_with_alias(id: Uuid, name: Option<&str>, graph_alias: Option<&str>) -> Result<BTreeSet<String>, String> {
    let mut out = BTreeSet::new();
    let id_tok = id.to_string();
    out.insert(id_tok.clone());

    if let Some(name) = name
        && let Some(tok) = identity_tokens::normalize_token(name)
    {
        if tok == id_tok {
            return Err(tok);
        }
        let _ = out.insert(tok);
    }

    if let Some(alias) = graph_alias
        && let Some(tok) = identity_tokens::normalize_token(alias)
    {
        if tok == id_tok {
            return Err(tok);
        }
        let _ = out.insert(tok);
    }

    Ok(out)
}

pub(super) async fn pipeline_identity_conflict(
    dir: &std::path::Path,
    requested_id: Uuid,
    requested_name: Option<&str>,
    requested_graph: &serde_json::Value,
) -> Result<Option<PipelineIdentityConflict>, axum::response::Response> {
    let requested_alias = pipeline_graph_alias(requested_graph);
    let requested_tokens = match pipeline_identity_token_set_with_alias(requested_id, requested_name, requested_alias) {
        Ok(set) => set,
        Err(tok) => {
            return Ok(Some(PipelineIdentityConflict { token: tok, existing_id: None, existing_name: None, existing_alias: None }));
        }
    };

    let mut entries = match fs::read_dir(dir).await {
        Ok(entries) => entries,
        Err(err) => return Err(map_io_error(err, "failed to read pipeline directory")),
    };

    loop {
        let entry = match entries.next_entry().await {
            Ok(Some(entry)) => entry,
            Ok(None) => break,
            Err(err) => return Err(map_io_error(err, "failed to read pipeline entry")),
        };

        let meta = match entry.metadata().await {
            Ok(meta) if meta.is_file() => meta,
            Ok(_) => continue,
            Err(err) => return Err(map_io_error(err, "failed to stat pipeline file")),
        };

        let Some(file_name) = entry.file_name().to_str().map(|s| s.to_string()) else {
            continue;
        };
        if !file_name.ends_with(".json") {
            continue;
        }

        let data = match fs::read_to_string(entry.path()).await {
            Ok(data) => data,
            Err(err) => return Err(map_io_error(err, "failed to read pipeline file")),
        };
        let Ok(doc) = serde_json::from_str::<PipelineDocument>(&data) else {
            continue;
        };
        if doc.id == requested_id {
            continue;
        }

        let existing_alias = pipeline_graph_alias(&doc.graph);
        let existing_tokens = match pipeline_identity_token_set_with_alias(doc.id, doc.name.as_deref(), existing_alias) {
            Ok(set) => set,
            Err(tok) => {
                warn!(pipeline_id = %doc.id, token = %tok, "pipeline has colliding identity tokens; treating as reserved for uniqueness checks");
                let mut set = BTreeSet::new();
                set.insert(doc.id.to_string());
                if let Some(name) = doc.name.as_deref().and_then(identity_tokens::normalize_token) {
                    let _ = set.insert(name);
                }
                if let Some(alias) = existing_alias.and_then(identity_tokens::normalize_token) {
                    let _ = set.insert(alias);
                }
                set
            }
        };

        if let Some(tok) = requested_tokens.intersection(&existing_tokens).next().cloned() {
            let _ = meta;
            return Ok(Some(PipelineIdentityConflict { token: tok, existing_id: Some(doc.id), existing_name: doc.name.clone(), existing_alias: existing_alias.map(|s| s.to_string()) }));
        }
    }

    Ok(None)
}

pub(super) async fn ensure_unique_pipeline_identity(dir: &std::path::Path, requested_id: Uuid, requested_name: Option<&str>, requested_graph: &serde_json::Value) -> Option<axum::response::Response> {
    match pipeline_identity_conflict(dir, requested_id, requested_name, requested_graph).await {
        Ok(None) => None,
        Ok(Some(conflict)) => {
            if conflict.existing_id.is_none() {
                return Some(
                    (StatusCode::CONFLICT, Json(PipelineError { error: format!("pipeline identity tokens collide within the requested pipeline: token=\"{}\"", conflict.token) })).into_response(),
                );
            }

            let existing_name = conflict.existing_name.as_deref().unwrap_or("");
            let existing_alias = conflict.existing_alias.as_deref().unwrap_or("");
            let mut details = Vec::new();
            if !existing_name.is_empty() {
                details.push(format!("name=\"{existing_name}\""));
            }
            if !existing_alias.is_empty() && existing_alias != existing_name {
                details.push(format!("alias=\"{existing_alias}\""));
            }
            let details = if details.is_empty() { "".to_string() } else { format!(" ({})", details.join(", ")) };
            let existing_id = conflict.existing_id.expect("existing pipeline id set");
            Some(
                (
                    StatusCode::CONFLICT,
                    Json(PipelineError { error: format!("pipeline identity token already in use: token=\"{}\" conflicts with pipeline id=\"{}\"{details}", conflict.token, existing_id) }),
                )
                    .into_response(),
            )
        }
        Err(resp) => Some(resp),
    }
}

#[cfg(test)]
mod tests {
    use super::super::graph_support::pipeline_graph_alias;
    use super::pipeline_identity_token_set_with_alias;
    use uuid::Uuid;

    #[test]
    fn pipeline_name_cannot_collide_with_id_token() {
        let id = Uuid::new_v4();
        let name = id.to_string().to_uppercase();
        let err = pipeline_identity_token_set_with_alias(id, Some(&name), None).unwrap_err();
        assert_eq!(err, id.to_string());
    }

    #[test]
    fn pipeline_graph_alias_participates_in_uniqueness() {
        let id = Uuid::new_v4();
        let graph = serde_json::json!({
            "nodes": [],
            "edges": [],
            "metadata": { "helios.pipeline.alias": "My Pipeline" }
        });
        let alias = pipeline_graph_alias(&graph).unwrap();
        assert_eq!(alias, "My Pipeline");

        let set = pipeline_identity_token_set_with_alias(id, None, Some(alias)).expect("token set");
        assert!(set.contains(&id.to_string()));
        assert!(set.contains("mypipeline"));
    }
}
