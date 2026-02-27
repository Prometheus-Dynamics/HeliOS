use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Default, Serialize, Deserialize, ToSchema)]
pub struct DeviceIdentity {
    pub id: Option<Uuid>,
    pub alias: Option<String>,
    pub hardware_id: Option<String>,
}

impl DeviceIdentity {
    pub fn new(id: Option<Uuid>, hardware_id: Option<String>, alias: Option<String>) -> Self {
        Self { id, alias, hardware_id }
    }

    pub fn with_id(id: Uuid) -> Self {
        Self { id: Some(id), alias: None, hardware_id: None }
    }

    pub fn hardware_id(&self) -> Option<&str> {
        self.hardware_id.as_deref()
    }
}
