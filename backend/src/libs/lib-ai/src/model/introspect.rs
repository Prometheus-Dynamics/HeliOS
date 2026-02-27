use super::{ModelFormat, ModelTensorMetadata, TensorQuantization};
use crate::backend::util::is_edge_tpu_compiled_model;
use crate::tensor::TensorElementType;

#[derive(Debug, Clone, Default)]
pub struct ModelInspection {
    pub inputs: Vec<ModelTensorMetadata>,
    pub outputs: Vec<ModelTensorMetadata>,
    pub suggested_tags: Vec<String>,
}

pub fn inspect_model(bytes: &[u8], format: &ModelFormat) -> ModelInspection {
    match format {
        #[cfg(feature = "backend-tflite")]
        ModelFormat::TensorFlowLite => inspect_tflite(bytes),
        _ => {
            let mut inspection = ModelInspection { suggested_tags: default_tags(format, bytes), ..ModelInspection::default() };
            add_runtime_tags(&mut inspection.suggested_tags, format, bytes);
            inspection
        }
    }
}

#[cfg(feature = "backend-tflite")]
fn inspect_tflite(bytes: &[u8]) -> ModelInspection {
    use tract_tflite::tflite;

    let mut inspection = ModelInspection { suggested_tags: default_tags(&ModelFormat::TensorFlowLite, bytes), ..ModelInspection::default() };

    if let Ok(model) = tflite::root_as_model(bytes)
        && let Some(subgraphs) = model.subgraphs()
        && !subgraphs.is_empty()
    {
        let subgraph = subgraphs.get(0);
        if let Some(tensors) = subgraph.tensors() {
            if let Some(inputs) = subgraph.inputs() {
                inspection.inputs = inputs.iter().map(|index| tensors.get(index as usize)).filter_map(|tensor| map_tensor_metadata(&tensor)).collect();
            }
            if let Some(outputs) = subgraph.outputs() {
                inspection.outputs = outputs.iter().map(|index| tensors.get(index as usize)).filter_map(|tensor| map_tensor_metadata(&tensor)).collect();
            }
        }
    }

    if is_detection_model(&inspection.outputs) && !inspection.suggested_tags.iter().any(|tag| tag == "detector") {
        inspection.suggested_tags.push("detector".into());
    }

    if is_vision_model(&inspection.inputs) && !inspection.suggested_tags.iter().any(|tag| tag == "vision") {
        inspection.suggested_tags.push("vision".into());
    }

    add_runtime_tags(&mut inspection.suggested_tags, &ModelFormat::TensorFlowLite, bytes);
    add_precision_tags(&mut inspection.suggested_tags, &inspection.inputs);

    inspection
}

fn default_tags(format: &ModelFormat, bytes: &[u8]) -> Vec<String> {
    let mut tags = Vec::new();
    match format {
        ModelFormat::TensorFlowLite => tags.push("tflite".into()),
        ModelFormat::Onnx => tags.push("onnx".into()),
        ModelFormat::Raw => tags.push("raw".into()),
    }

    if matches!(format, ModelFormat::TensorFlowLite) && is_edge_tpu_compiled_model(bytes) {
        tags.push("edge-tpu".into());
    }

    tags
}

#[cfg(feature = "backend-tflite")]
fn map_tensor_metadata(tensor: &tract_tflite::tflite::Tensor) -> Option<ModelTensorMetadata> {
    let element_type = map_tensor_type(tensor.type_())?;
    let shape = tensor.shape().map(|shape| shape.iter().map(|dim| dim.max(1) as usize).collect()).unwrap_or_default();
    let name = tensor.name().map(|name| name.to_string()).filter(|name| !name.is_empty());
    let quantization = tensor.quantization().and_then(map_quantization_params);
    Some(ModelTensorMetadata { name, element_type, shape, quantization })
}

#[cfg(feature = "backend-tflite")]
fn map_tensor_type(value: tract_tflite::tflite::TensorType) -> Option<TensorElementType> {
    use tract_tflite::tflite::TensorType;
    match value {
        TensorType::FLOAT32 => Some(TensorElementType::F32),
        TensorType::FLOAT16 => Some(TensorElementType::F16),
        TensorType::INT32 => Some(TensorElementType::I32),
        TensorType::INT16 => Some(TensorElementType::I16),
        TensorType::INT8 => Some(TensorElementType::I8),
        TensorType::UINT8 => Some(TensorElementType::U8),
        _ => None,
    }
}

#[cfg(feature = "backend-tflite")]
fn map_quantization_params(params: tract_tflite::tflite::QuantizationParameters) -> Option<TensorQuantization> {
    let zero_point = params.zero_point().map(|values| values.iter().collect::<Vec<_>>()).unwrap_or_default();
    let scale = params.scale().map(|values| values.iter().collect::<Vec<_>>()).unwrap_or_default();

    if zero_point.is_empty() && scale.is_empty() { None } else { Some(TensorQuantization { zero_point, scale }) }
}

fn is_vision_model(inputs: &[ModelTensorMetadata]) -> bool {
    inputs.iter().any(|tensor| tensor.shape.len() >= 3 && matches!(tensor.shape.last(), Some(3) | Some(4)))
}

fn is_detection_model(outputs: &[ModelTensorMetadata]) -> bool {
    if outputs.len() < 4 {
        return false;
    }

    let boxes = &outputs[0].shape;
    let scores = &outputs[1].shape;
    let classes = &outputs[2].shape;
    let num = &outputs[3].shape;

    boxes.len() == 3 && boxes.last() == Some(&4) && scores.len() >= 2 && classes.len() >= 2 && num.len() == 1
}

fn add_runtime_tags(tags: &mut Vec<String>, format: &ModelFormat, bytes: &[u8]) {
    let edge_tpu = matches!(format, ModelFormat::TensorFlowLite) && is_edge_tpu_compiled_model(bytes);
    if edge_tpu {
        push_tag(tags, "runtime:coral");
        push_tag(tags, "requires-edge-tpu");
    } else {
        match format {
            ModelFormat::TensorFlowLite | ModelFormat::Onnx | ModelFormat::Raw => {
                push_tag(tags, "runtime:cpu");
            }
        }
    }
}

fn add_precision_tags(tags: &mut Vec<String>, inputs: &[ModelTensorMetadata]) {
    let mut has_i8 = false;
    let mut has_u8 = false;
    let mut has_f32 = false;
    let mut has_f16 = false;
    let mut quantized = false;

    for tensor in inputs {
        match tensor.element_type {
            TensorElementType::I8 => has_i8 = true,
            TensorElementType::U8 => has_u8 = true,
            TensorElementType::F32 => has_f32 = true,
            TensorElementType::F16 => has_f16 = true,
            _ => {}
        }
        if tensor.quantization.is_some() {
            quantized = true;
        }
    }

    if quantized || has_i8 || has_u8 {
        push_tag(tags, "quantized");
    }
    if has_i8 {
        push_tag(tags, "precision:int8");
    }
    if has_u8 {
        push_tag(tags, "precision:uint8");
    }
    if has_f32 {
        push_tag(tags, "precision:f32");
    }
    if has_f16 {
        push_tag(tags, "precision:f16");
    }
}

fn push_tag(tags: &mut Vec<String>, tag: &str) {
    if tags.iter().any(|existing| existing.eq_ignore_ascii_case(tag)) {
        return;
    }
    tags.push(tag.to_string());
}
