//! Inference helpers for Daedalus-backed AI nodes.

use crate::{NormalizedBoundingBox, VisionDetection2D};
use bytemuck::{cast_slice, cast_slice_mut};
use image::DynamicImage;
use lib_cv::image::resize::resize_fast;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use tracing::{info, warn};

use crate::backend::AiModel;
#[cfg(feature = "backend-coral")]
use crate::backend::coral;
use crate::backend::util::is_edge_tpu_compiled_model;
use crate::error::AiError;
use crate::model::{ModelFormat, ModelId, ModelLoadRequest, ModelMetadata, ModelSource, TensorQuantization};
use crate::runtime::{BackendSelection, Runtime};
use crate::storage::{default_model_dir, list_models, load_model};
use crate::{BACKEND_REGISTRY, Tensor, TensorElementType};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum BackendPreference {
    Auto,
    Cpu,
    Coral,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InferenceConfig {
    pub model_id: ModelId,
    pub backend: BackendPreference,
    pub device_path: Option<String>,
    pub min_score: f32,
    pub max_results: usize,
    pub normalize: bool,
}

pub struct ModelRuntimeState {
    pub config: InferenceConfig,
    pub metadata: ModelMetadata,
    pub model: std::sync::Arc<dyn AiModel>,
}

pub struct ModelRuntimeHandle {
    registry: &'static crate::registry::Registry<dyn crate::backend::AiBackend>,
    state: Option<std::sync::Arc<ModelRuntimeState>>,
}

impl ModelRuntimeHandle {
    pub fn new(node_id: &str) -> Self {
        let _ = node_id;
        Self { registry: &BACKEND_REGISTRY, state: None }
    }

    pub fn ensure_model_blocking(&mut self, config: &InferenceConfig) -> Result<std::sync::Arc<ModelRuntimeState>, AiError> {
        let reload = self.state.as_ref().map(|s| s.config != *config).unwrap_or(true);
        if reload {
            let state = std::sync::Arc::new(self.load_model_blocking(config)?);
            self.state = Some(state);
        }
        Ok(self.state.as_ref().expect("state").clone())
    }

    fn load_model_blocking(&self, config: &InferenceConfig) -> Result<ModelRuntimeState, AiError> {
        let model_dir = default_model_dir();
        let stored = load_model(&model_dir, &config.model_id).map_err(|err| AiError::ModelLoadFailed { reason: err.to_string() })?;
        let bytes = std::fs::read(&stored.artifact_path).map_err(|err| AiError::ModelLoadFailed { reason: format!("unable to read model at {}: {err}", stored.artifact_path.display()) })?;
        let is_edge_tpu = is_edge_tpu_compiled_model(&bytes);

        match config.backend {
            BackendPreference::Cpu if is_edge_tpu => {
                return Err(AiError::unsupported("CPU backend selected but the model is Edge TPU-compiled. Choose a CPU-compatible .tflite or set device_path to Coral."));
            }
            BackendPreference::Coral if !is_edge_tpu => {
                return Err(AiError::unsupported("Coral backend selected but the model is not Edge TPU-compiled. Compile with edgetpu_compiler or choose CPU backend."));
            }
            _ => {}
        }

        let mut metadata = stored.metadata.clone();
        enrich_metadata_quantization(&mut metadata, &stored.format, &stored.artifact_path);
        let request = ModelLoadRequest { id: stored.id.clone(), format: stored.format.clone(), source: ModelSource::File(stored.artifact_path.clone()), metadata: metadata.clone() };
        let runtime = Runtime::with_selection(self.registry, backend_selection(&config.backend));
        let loaded = runtime.load_model_blocking(&request)?;
        if let Some(coral_model) = loaded.model.as_any().downcast_ref::<crate::backend::coral::CoralModel>() {
            coral_model.set_preferred_device(config.device_path.clone());
        }
        info!(
            backend = %loaded.backend_name,
            model_id = %config.model_id.0,
            preference = ?config.backend,
            device_path = ?config.device_path,
            "ai model loaded"
        );
        Ok(ModelRuntimeState { config: config.clone(), metadata, model: loaded.model })
    }
}

pub fn backend_selection(preference: &BackendPreference) -> BackendSelection {
    match preference {
        BackendPreference::Cpu => BackendSelection { preferred: vec!["TfliteCpuBackend".into(), "OnnxCpuBackend".into()] },
        BackendPreference::Coral => BackendSelection { preferred: vec!["CoralBackend".into(), "TfliteCpuBackend".into()] },
        BackendPreference::Auto => BackendSelection { preferred: vec!["CoralBackend".into(), "TfliteCpuBackend".into(), "OnnxCpuBackend".into()] },
    }
}

pub fn inference_model_variants() -> Vec<String> {
    model_variant_values()
}

pub fn inference_device_variants() -> Vec<String> {
    device_variant_values()
}

fn model_variant_values() -> Vec<String> {
    match list_models(default_model_dir()) {
        Ok(models) => models
            .into_iter()
            .map(|model| {
                let id = model.id.0.to_string();
                let base = model.metadata.display_name.as_deref().map(str::trim).filter(|value| !value.is_empty()).map(|value| value.to_string()).unwrap_or_else(|| id.clone());
                let profile = describe_model_profile(&model);
                let display = if let Some(precision) = profile.precision { format!("{} · {} · {base}", profile.runtime, precision) } else { format!("{} · {base}", profile.runtime) };
                format!("{display}|{id}")
            })
            .collect(),
        Err(err) => {
            warn!(error = %err, "failed to list AI models for inference node");
            Vec::new()
        }
    }
}

fn device_variant_values() -> Vec<String> {
    let mut variants = vec!["Auto|auto".to_string(), "CPU|cpu".to_string()];
    let mut seen = HashSet::new();
    seen.insert("auto".to_string());
    seen.insert("cpu".to_string());

    add_coral_variants(&mut variants, &mut seen);

    variants
}

struct ModelProfile {
    runtime: &'static str,
    precision: Option<&'static str>,
}

fn describe_model_profile(model: &crate::storage::StoredModel) -> ModelProfile {
    let tags = model.metadata.tags.iter().map(|tag| tag.to_ascii_lowercase()).collect::<Vec<_>>();
    let mut is_edge_tpu = tags.iter().any(|tag| tag == "edge-tpu" || tag == "edgetpu" || tag == "runtime:coral");
    if !is_edge_tpu && let Ok(bytes) = std::fs::read(&model.artifact_path) {
        is_edge_tpu = is_edge_tpu_compiled_model(&bytes);
    }

    let runtime = if is_edge_tpu { "TPU" } else { "CPU" };

    let precision_tag = tags.iter().find_map(|tag| {
        if tag == "precision:int8" || tag == "int8" {
            Some("INT8")
        } else if tag == "precision:uint8" || tag == "uint8" {
            Some("UINT8")
        } else if tag == "precision:f16" || tag == "float16" || tag == "f16" {
            Some("F16")
        } else if tag == "precision:f32" || tag == "float32" || tag == "f32" {
            Some("F32")
        } else {
            None
        }
    });

    let precision = precision_tag.or_else(|| {
        model.metadata.inputs.iter().find_map(|tensor| match tensor.element_type {
            TensorElementType::I8 => Some("INT8"),
            TensorElementType::U8 => Some("UINT8"),
            TensorElementType::F16 => Some("F16"),
            TensorElementType::F32 => Some("F32"),
            _ => None,
        })
    });

    ModelProfile { runtime, precision }
}

#[cfg(feature = "backend-coral")]
fn add_coral_variants(variants: &mut Vec<String>, seen: &mut HashSet<String>) {
    match coral::enumerate_usb_devices() {
        Ok(devices) => {
            for device in devices {
                let Some(path) = device.path.as_deref().map(str::trim).filter(|value| !value.is_empty()) else {
                    continue;
                };
                if !seen.insert(path.to_string()) {
                    continue;
                }
                variants.push(format!("Coral USB {path}|{path}"));
            }
        }
        Err(err) => {
            warn!(error = %err, "failed to list Coral devices for inference node");
        }
    }
}

#[cfg(not(feature = "backend-coral"))]
fn add_coral_variants(_variants: &mut Vec<String>, _seen: &mut HashSet<String>) {}

pub fn encode_image_tensor_from_value(frame: &DynamicImage, config: &InferenceConfig) -> Result<Tensor, String> {
    let stored = load_model(default_model_dir(), &config.model_id).map_err(|e| e.to_string())?;
    let input_meta = stored.metadata.inputs.first().ok_or_else(|| "model metadata missing input description".to_string())?;
    let (height, width, channels) = resolve_shape(&input_meta.shape).ok_or_else(|| "unsupported input tensor shape".to_string())?;
    let image = resize_fast(frame, width as u32, height as u32);
    let channels = channels.min(3);
    let tensor_name = input_meta.name.clone().unwrap_or_else(|| "input_0".into());

    let element_type = input_meta.element_type.clone();
    match element_type {
        TensorElementType::U8 => {
            let bytes = image.to_rgb8().into_raw();
            Ok(Tensor::new(tensor_name, TensorElementType::U8, input_meta.shape.clone()).with_bytes(bytes))
        }
        TensorElementType::F32 => {
            let rgb = image.to_rgb8().into_raw();
            let pixel_count = rgb.len() / 3;
            let mut bytes = vec![0u8; pixel_count * channels * std::mem::size_of::<f32>()];
            let floats: &mut [f32] = cast_slice_mut(bytes.as_mut_slice());
            let scale = if config.normalize { 1.0 / 255.0 } else { 1.0 };

            floats.par_chunks_mut(channels).enumerate().for_each(|(idx, chunk)| {
                let src = &rgb[idx * 3..idx * 3 + 3];
                for c in 0..channels {
                    chunk[c] = f32::from(src[c]) * scale;
                }
            });

            Ok(Tensor::new(tensor_name, TensorElementType::F32, input_meta.shape.clone()).with_bytes(bytes))
        }
        other => Err(format!("unsupported input element type: {other:?}")),
    }
}

pub fn resolve_shape(shape: &[usize]) -> Option<(usize, usize, usize)> {
    match shape {
        [h, w, c] => Some((*h, *w, *c)),
        [1, h, w, c] if *h > 4 && *w > 4 => Some((*h, *w, *c)),
        [1, c, h, w] => Some((*h, *w, *c)),
        _ => None,
    }
}

pub fn enrich_metadata_quantization(metadata: &mut ModelMetadata, format: &ModelFormat, artifact: &std::path::Path) {
    if matches!(format, ModelFormat::TensorFlowLite)
        && let Ok(bytes) = std::fs::read(artifact)
    {
        let meta = crate::model::introspect::inspect_model(&bytes, format);
        metadata.inputs = meta.inputs;
        metadata.outputs = meta.outputs;
    }
}

pub fn parse_detections(outputs: Vec<Tensor>, metadata: &ModelMetadata, min_score: f32, max_results: usize, node_id: &str) -> Vec<VisionDetection2D> {
    let mut detections = Vec::new();
    let parsed = locate_output_tensors(&outputs, metadata);
    let (boxes_tensor, boxes_index) = match parsed.boxes {
        Some(value) => value,
        None => return detections,
    };
    let (scores_tensor, scores_index) = match parsed.scores {
        Some(value) => value,
        None => return detections,
    };

    let boxes_quant = tensor_output_quantization(metadata, boxes_index);
    let scores_quant = tensor_output_quantization(metadata, scores_index);

    let label_count = metadata.labels.len();
    let max_candidates = tensor_value_len(boxes_tensor, node_id).unwrap_or(0) / 4;
    if max_candidates == 0 {
        return detections;
    }
    let count = parsed.count.and_then(|(tensor, index)| tensor_segment_as_f32(tensor, tensor_output_quantization(metadata, index), node_id, 0, 1).ok()).and_then(|values| values.first().copied());

    let to_process = count.map(|value| value as usize).unwrap_or(max_candidates).min(max_candidates).min(max_results);

    if to_process == 0 {
        return detections;
    }

    let boxes = tensor_segment_as_f32(boxes_tensor, boxes_quant, node_id, 0, to_process * 4).unwrap_or_default();
    let mut scores = tensor_segment_as_f32(scores_tensor, scores_quant, node_id, 0, to_process).unwrap_or_default();
    let mut class_values = parsed.classes.and_then(|(tensor, index)| tensor_segment_as_f32(tensor, tensor_output_quantization(metadata, index), node_id, 0, to_process).ok());
    if let Some(values) = class_values.as_mut()
        && scores.len() == values.len()
        && looks_like_class_ids(&scores, label_count)
        && looks_like_probabilities(values)
    {
        std::mem::swap(&mut scores, values);
    }
    let classes = class_values.map(|values| values.into_iter().map(|value| value.round().max(0.0) as u32).collect::<Vec<_>>());

    for idx in 0..to_process {
        let score = scores.get(idx).copied().unwrap_or_default();
        if score < min_score {
            continue;
        }
        let base = idx * 4;
        if boxes.len() < base + 4 {
            break;
        }
        let bbox = NormalizedBoundingBox { ymin: boxes[base], xmin: boxes[base + 1], ymax: boxes[base + 2], xmax: boxes[base + 3] }.clamp();
        let class_id = classes.as_ref().and_then(|values| values.get(idx).copied());
        let label = class_id.and_then(|id| metadata.labels.get(id as usize).cloned()).filter(|name| !name.is_empty());
        detections.push(VisionDetection2D { bbox, score, label, class_id });
    }

    detections
}

struct OutputSelection<'a> {
    boxes: Option<(&'a Tensor, usize)>,
    scores: Option<(&'a Tensor, usize)>,
    classes: Option<(&'a Tensor, usize)>,
    count: Option<(&'a Tensor, usize)>,
}

fn locate_output_tensors<'a>(outputs: &'a [Tensor], metadata: &'a ModelMetadata) -> OutputSelection<'a> {
    if metadata.outputs.is_empty() {
        return fallback_output_selection(outputs);
    }
    let mut boxes = None;
    let mut scores = None;
    let mut classes = None;
    let mut count = None;

    for (idx, meta) in metadata.outputs.iter().enumerate() {
        let name = meta.name.as_deref().unwrap_or("").to_ascii_lowercase();
        if boxes.is_none() && name.contains("box") {
            boxes = outputs.get(idx).map(|tensor| (tensor, idx));
        } else if scores.is_none() && name.contains("score") {
            scores = outputs.get(idx).map(|tensor| (tensor, idx));
        } else if classes.is_none() && (name.contains("class") || name.contains("label")) {
            classes = outputs.get(idx).map(|tensor| (tensor, idx));
        } else if count.is_none() && (name.contains("count") || name.contains("num")) {
            count = outputs.get(idx).map(|tensor| (tensor, idx));
        }
    }

    if boxes.is_none() || scores.is_none() { fallback_output_selection(outputs) } else { OutputSelection { boxes, scores, classes, count } }
}

fn fallback_output_selection<'a>(outputs: &'a [Tensor]) -> OutputSelection<'a> {
    OutputSelection {
        boxes: outputs.first().map(|tensor| (tensor, 0)),
        classes: outputs.get(1).map(|tensor| (tensor, 1)),
        scores: outputs.get(2).map(|tensor| (tensor, 2)),
        count: outputs.get(3).map(|tensor| (tensor, 3)),
    }
}

fn tensor_segment_as_f32(tensor: &Tensor, quantization: Option<&TensorQuantization>, node_id: &str, offset: usize, count: usize) -> Result<Vec<f32>, String> {
    let bytes = tensor.bytes.as_ref().ok_or_else(|| format!("{node_id}: tensor {} missing data", tensor.name))?;
    match tensor.element_type {
        TensorElementType::F32 => {
            let values = cast_slice::<u8, f32>(bytes);
            let (start, end) = clamp_segment(values.len(), offset, count);
            Ok(values[start..end].to_vec())
        }
        TensorElementType::U8 => {
            let (start, end) = clamp_segment(bytes.len(), offset, count);
            Ok(bytes[start..end].iter().map(|value| dequantize_u8(*value, quantization)).collect())
        }
        TensorElementType::I8 => {
            let values = cast_slice::<u8, i8>(bytes);
            let (start, end) = clamp_segment(values.len(), offset, count);
            Ok(values[start..end].iter().copied().map(|value| dequantize_i8(value, quantization)).collect())
        }
        TensorElementType::I16 => {
            let values = cast_slice::<u8, i16>(bytes);
            let (start, end) = clamp_segment(values.len(), offset, count);
            Ok(values[start..end].iter().copied().map(|value| dequantize_i16(value, quantization)).collect())
        }
        TensorElementType::I32 => {
            let values = cast_slice::<u8, i32>(bytes);
            let (start, end) = clamp_segment(values.len(), offset, count);
            Ok(values[start..end].iter().copied().map(|value| dequantize_i32(value, quantization)).collect())
        }
        ref other => Err(format!("{node_id}: unsupported tensor element type: {other:?}")),
    }
}

fn tensor_value_len(tensor: &Tensor, node_id: &str) -> Result<usize, String> {
    let bytes = tensor.bytes.as_ref().ok_or_else(|| format!("{node_id}: tensor {} missing data", tensor.name))?;
    let element_size = match tensor.element_type {
        TensorElementType::F32 => std::mem::size_of::<f32>(),
        TensorElementType::U8 => std::mem::size_of::<u8>(),
        TensorElementType::I8 => std::mem::size_of::<i8>(),
        TensorElementType::I16 => std::mem::size_of::<i16>(),
        TensorElementType::I32 => std::mem::size_of::<i32>(),
        ref other => return Err(format!("{node_id}: unsupported tensor element type: {other:?}")),
    };
    Ok(bytes.len() / element_size)
}

fn clamp_segment(len: usize, offset: usize, count: usize) -> (usize, usize) {
    if len == 0 || count == 0 {
        return (len.min(offset), len.min(offset));
    }
    let start = offset.min(len);
    let end = (start + count).min(len);
    (start, end)
}

fn looks_like_probabilities(values: &[f32]) -> bool {
    let mut finite = 0;
    let mut in_range = 0;
    let mut fractional = 0;

    for &value in values {
        if !value.is_finite() {
            continue;
        }
        finite += 1;
        if (0.0..=1.0).contains(&value) {
            in_range += 1;
            if (value - value.round()).abs() > 0.05 {
                fractional += 1;
            }
        }
    }

    finite >= 4 && in_range * 5 >= finite * 4 && fractional > 0
}

fn looks_like_class_ids(values: &[f32], label_count: usize) -> bool {
    let mut finite = 0;
    let mut integerish = 0;
    let mut above_one = 0;
    let upper_bound = (label_count.max(1) as f32) + 5.0;

    for &value in values {
        if !value.is_finite() {
            continue;
        }
        finite += 1;
        let rounded = value.round();
        if (value - rounded).abs() < 0.01 && rounded >= -1.0 && rounded <= upper_bound {
            integerish += 1;
            if rounded > 1.0 {
                above_one += 1;
            }
        }
    }

    finite >= 4 && integerish * 5 >= finite * 4 && above_one > 0
}

fn tensor_output_quantization(metadata: &ModelMetadata, output_index: usize) -> Option<&TensorQuantization> {
    metadata.outputs.get(output_index).and_then(|meta| meta.quantization.as_ref())
}

fn quantization_params(q: &TensorQuantization) -> (f32, f32) {
    let zero = q.zero_point.first().copied().unwrap_or(0) as f32;
    let scale = q.scale.first().copied().unwrap_or(1.0);
    (zero, scale)
}

fn dequantize_u8(value: u8, quantization: Option<&TensorQuantization>) -> f32 {
    if let Some(q) = quantization {
        let (zero, scale) = quantization_params(q);
        (f32::from(value) - zero) * scale
    } else {
        value as f32
    }
}

fn dequantize_i8(value: i8, quantization: Option<&TensorQuantization>) -> f32 {
    if let Some(q) = quantization {
        let (zero, scale) = quantization_params(q);
        (value as f32 - zero) * scale
    } else {
        value as f32
    }
}

fn dequantize_i16(value: i16, quantization: Option<&TensorQuantization>) -> f32 {
    if let Some(q) = quantization {
        let (zero, scale) = quantization_params(q);
        (value as f32 - zero) * scale
    } else {
        value as f32
    }
}

fn dequantize_i32(value: i32, quantization: Option<&TensorQuantization>) -> f32 {
    if let Some(q) = quantization {
        let (zero, scale) = quantization_params(q);
        (value as f32 - zero) * scale
    } else {
        value as f32
    }
}

pub fn encode_tensor_from_bytes(bytes: &[u8], element_type: TensorElementType, shape: Vec<usize>, name: String) -> Tensor {
    Tensor { name, element_type, shape, bytes: Some(bytes.to_vec()) }
}
