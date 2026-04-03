use serde::{Deserialize, Serialize};
#[cfg(feature = "schema")]
use utoipa::ToSchema;

pub type TensorShape = Vec<usize>;

#[cfg_attr(feature = "schema", derive(ToSchema))]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum TensorElementType {
    U8,
    I8,
    I16,
    I32,
    F16,
    F32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tensor {
    pub name: String,
    pub element_type: TensorElementType,
    pub shape: TensorShape,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bytes: Option<Vec<u8>>,
}

impl Tensor {
    pub fn new(name: impl Into<String>, element_type: TensorElementType, shape: TensorShape) -> Self {
        Self { name: name.into(), element_type, shape, bytes: None }
    }

    pub fn with_bytes(mut self, bytes: Vec<u8>) -> Self {
        self.bytes = Some(bytes);
        self
    }
}
