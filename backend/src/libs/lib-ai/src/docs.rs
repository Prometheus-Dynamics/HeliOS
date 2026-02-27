use serde::{Deserialize, Serialize};
#[cfg(feature = "schema")]
use utoipa::ToSchema;

#[cfg_attr(feature = "schema", derive(ToSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackendDoc {
    pub display_name: String,
    pub summary: String,
    pub description: String,
    pub tags: Vec<String>,
}

pub trait DocumentedBackend {
    fn docs() -> BackendDoc;
}
