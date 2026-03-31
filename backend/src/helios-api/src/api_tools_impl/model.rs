use helios_peripherals::{AiModelTensorMetadata, AiTensorElementType, AiTensorQuantization};

use crate::api_tools_protocol::ToolModelInspection;

pub(super) fn inspect_model(model_bytes: &[u8], format: helios_peripherals::AiModelFormat) -> ToolModelInspection {
    let lib_format = match format {
        helios_peripherals::AiModelFormat::TensorFlowLite => lib_ai::model::ModelFormat::TensorFlowLite,
        helios_peripherals::AiModelFormat::Onnx => lib_ai::model::ModelFormat::Onnx,
        helios_peripherals::AiModelFormat::Raw => lib_ai::model::ModelFormat::Raw,
    };
    let inspection = lib_ai::model::introspect::inspect_model(model_bytes, &lib_format);
    tool_model_inspection_from_lib(inspection)
}

fn tool_model_inspection_from_lib(inspection: lib_ai::model::introspect::ModelInspection) -> ToolModelInspection {
    ToolModelInspection {
        suggested_tags: inspection.suggested_tags,
        inputs: inspection.inputs.into_iter().map(ai_model_tensor_metadata_from_lib).collect(),
        outputs: inspection.outputs.into_iter().map(ai_model_tensor_metadata_from_lib).collect(),
    }
}

fn ai_model_tensor_metadata_from_lib(tensor: lib_ai::model::ModelTensorMetadata) -> AiModelTensorMetadata {
    AiModelTensorMetadata {
        name: tensor.name,
        element_type: match tensor.element_type {
            lib_ai::tensor::TensorElementType::U8 => AiTensorElementType::U8,
            lib_ai::tensor::TensorElementType::I8 => AiTensorElementType::I8,
            lib_ai::tensor::TensorElementType::I16 => AiTensorElementType::I16,
            lib_ai::tensor::TensorElementType::I32 => AiTensorElementType::I32,
            lib_ai::tensor::TensorElementType::F16 => AiTensorElementType::F16,
            lib_ai::tensor::TensorElementType::F32 => AiTensorElementType::F32,
        },
        shape: tensor.shape,
        quantization: tensor.quantization.map(|quantization| AiTensorQuantization { zero_point: quantization.zero_point, scale: quantization.scale }),
    }
}
