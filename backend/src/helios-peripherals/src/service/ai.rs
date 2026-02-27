use lib_ai::model::ModelId;

use crate::dto::{AiModelDescriptor, AiModelInventory, AiModelUpload};
use crate::error::{Error, Result};

use super::SensorsService;

impl SensorsService {
    pub async fn ai_inventory(&self) -> Result<AiModelInventory> {
        let models = self.ai.list_models().await?;
        Ok(AiModelInventory { models, max_upload_bytes: None })
    }

    pub async fn upload_ai_model(&self, request: AiModelUpload) -> Result<AiModelDescriptor> {
        self.ai.upload_model(request).await
    }

    pub async fn delete_ai_model(&self, model_id: ModelId) -> Result<()> {
        if self.ai.delete_model(&model_id).await? { Ok(()) } else { Err(Error::InvalidState(format!("model {} not found", model_id.0))) }
    }
}
