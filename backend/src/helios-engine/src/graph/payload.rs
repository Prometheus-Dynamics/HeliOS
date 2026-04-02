use daedalus::data::model::TypeExpr as DaedalusTypeExpr;
use daedalus::data::model::Value as DaedalusValue;
use daedalus::data::model::ValueType as DaedalusValueType;
use daedalus::runtime::RuntimeValue as DaedalusEdgePayload;
use lib_cv::modules::aruco::ArucoDetection2D;
use serde_json::Value;
use std::any::Any;
use std::collections::BTreeMap;
use std::sync::Arc;

use super::runtime_json::json_to_daedalus_value;

pub(super) fn is_image_payload(ty: &DaedalusTypeExpr) -> bool {
    match ty {
        DaedalusTypeExpr::Opaque(name) => {
            let lower = name.to_ascii_lowercase();
            lower == "image" || lower.starts_with("image:")
        }
        DaedalusTypeExpr::Optional(inner) => is_image_payload(inner.as_ref()),
        _ => false,
    }
}

fn is_grayscale_image_payload(ty: &DaedalusTypeExpr) -> bool {
    match ty {
        DaedalusTypeExpr::Opaque(name) => {
            let lower = name.to_ascii_lowercase();
            lower == "image:gray8" || lower == "image:graya8"
        }
        DaedalusTypeExpr::Optional(inner) => is_grayscale_image_payload(inner.as_ref()),
        _ => false,
    }
}

pub(super) fn node_requires_color_input(node_id: &str) -> bool {
    let lower = node_id.to_ascii_lowercase();
    lower.contains(":color:")
}

fn grayscale_compatible_dynamic_preview_port(port: &str) -> bool {
    matches!(port.trim().to_ascii_lowercase().as_str(), "overlay")
}

pub(super) fn preview_port_accepts_grayscale_input(port: &str, host_output_port_types: &BTreeMap<String, DaedalusTypeExpr>) -> bool {
    let key = port.to_ascii_lowercase();
    match host_output_port_types.get(&key) {
        Some(ty) if is_grayscale_image_payload(ty) => true,
        Some(ty) if is_image_payload(ty) && grayscale_compatible_dynamic_preview_port(&key) => true,
        None => grayscale_compatible_dynamic_preview_port(&key),
        _ => false,
    }
}

fn host_output_image_port_accepts_grayscale_input(port: &str, ty: &DaedalusTypeExpr) -> bool {
    is_grayscale_image_payload(ty) || (is_image_payload(ty) && grayscale_compatible_dynamic_preview_port(port))
}

pub(super) fn graph_prefers_grayscale_input(graph_has_color_sensitive_nodes: bool, preview_ports: &[String], host_output_port_types: &BTreeMap<String, DaedalusTypeExpr>) -> bool {
    if graph_has_color_sensitive_nodes {
        return false;
    }

    if preview_ports.iter().any(|port| !preview_port_accepts_grayscale_input(port, host_output_port_types)) {
        return false;
    }

    host_output_port_types.iter().filter(|(_, ty)| is_image_payload(ty)).all(|(port, ty)| host_output_image_port_accepts_grayscale_input(port, ty))
}

pub(super) fn is_aruco_detections_payload(ty: &DaedalusTypeExpr) -> bool {
    match ty {
        DaedalusTypeExpr::List(inner) => match inner.as_ref() {
            DaedalusTypeExpr::Opaque(name) => name.eq_ignore_ascii_case("cv:aruco_detection_2d"),
            other => is_aruco_detections_payload(other),
        },
        DaedalusTypeExpr::Optional(inner) => is_aruco_detections_payload(inner.as_ref()),
        _ => false,
    }
}

fn unwrap_nested_runtime_any<'a>(mut any: &'a (dyn Any + Send + Sync)) -> &'a (dyn Any + Send + Sync) {
    loop {
        if let Some(inner) = any.downcast_ref::<Arc<dyn Any + Send + Sync>>() {
            any = inner.as_ref();
            continue;
        }
        if let Some(inner) = any.downcast_ref::<Box<dyn Any + Send + Sync>>() {
            any = inner.as_ref();
            continue;
        }
        if let Some(inner) = any.downcast_ref::<Arc<Box<dyn Any + Send + Sync>>>() {
            any = inner.as_ref().as_ref();
            continue;
        }
        if let Some(inner) = any.downcast_ref::<Box<Arc<dyn Any + Send + Sync>>>() {
            any = inner.as_ref().as_ref();
            continue;
        }
        return any;
    }
}

pub(super) fn decode_runtime_value_as_aruco_detections(payload: &DaedalusEdgePayload) -> Option<Arc<Vec<ArucoDetection2D>>> {
    let DaedalusEdgePayload::Any(any) = payload else {
        return None;
    };
    let any = unwrap_nested_runtime_any(any.as_ref());
    any.downcast_ref::<Arc<Vec<ArucoDetection2D>>>().cloned().or_else(|| any.downcast_ref::<Vec<ArucoDetection2D>>().map(|detections| Arc::new(detections.clone())))
}

fn int_value(value: i64) -> DaedalusValue {
    DaedalusValue::Int(value)
}

fn float_value(value: f64) -> Option<DaedalusValue> {
    if !value.is_finite() {
        return None;
    }
    Some(DaedalusValue::Float(value))
}

fn string_value(raw: String) -> DaedalusValue {
    let parsed = serde_json::from_str::<Value>(&raw).unwrap_or(Value::String(raw));
    json_to_daedalus_value(&parsed)
}

fn bytes_value(bytes: Vec<u8>) -> DaedalusValue {
    DaedalusValue::Bytes(bytes.into())
}

pub(super) fn decode_runtime_value_fallback(payload: &DaedalusEdgePayload, port_type: Option<&DaedalusTypeExpr>) -> Option<DaedalusValue> {
    match payload {
        DaedalusEdgePayload::Value(value) => Some(value.clone()),
        DaedalusEdgePayload::Bytes(bytes) => Some(bytes_value(bytes.as_ref().to_vec())),
        DaedalusEdgePayload::Any(any) => {
            let any = unwrap_nested_runtime_any(any.as_ref());
            if let Some(json) = any.downcast_ref::<Value>() {
                return Some(json_to_daedalus_value(json));
            }
            if let Some(raw) = any.downcast_ref::<String>() {
                return Some(string_value(raw.clone()));
            }
            if let Some(raw) = any.downcast_ref::<Arc<String>>() {
                return Some(string_value((**raw).clone()));
            }
            if let Some(bytes) = any.downcast_ref::<Vec<u8>>() {
                return Some(bytes_value(bytes.clone()));
            }
            if let Some(bytes) = any.downcast_ref::<Arc<[u8]>>() {
                return Some(bytes_value(bytes.as_ref().to_vec()));
            }

            match port_type {
                Some(DaedalusTypeExpr::Scalar(DaedalusValueType::Bool)) => any.downcast_ref::<bool>().copied().map(DaedalusValue::Bool),
                Some(DaedalusTypeExpr::Scalar(DaedalusValueType::F32)) | Some(DaedalusTypeExpr::Scalar(DaedalusValueType::Float)) => {
                    any.downcast_ref::<f64>().copied().and_then(float_value).or_else(|| any.downcast_ref::<f32>().copied().and_then(|value| float_value(f64::from(value))))
                }
                Some(DaedalusTypeExpr::Scalar(DaedalusValueType::I32)) | Some(DaedalusTypeExpr::Scalar(DaedalusValueType::U32)) | Some(DaedalusTypeExpr::Scalar(DaedalusValueType::Int)) => any
                    .downcast_ref::<i64>()
                    .copied()
                    .map(int_value)
                    .or_else(|| any.downcast_ref::<i32>().copied().map(|value| int_value(i64::from(value))))
                    .or_else(|| any.downcast_ref::<u32>().copied().map(|value| int_value(i64::from(value))))
                    .or_else(|| any.downcast_ref::<u64>().copied().and_then(|value| i64::try_from(value).ok()).map(int_value))
                    .or_else(|| any.downcast_ref::<usize>().copied().and_then(|value| i64::try_from(value).ok()).map(int_value))
                    .or_else(|| any.downcast_ref::<u16>().copied().map(|value| int_value(i64::from(value))))
                    .or_else(|| any.downcast_ref::<u8>().copied().map(|value| int_value(i64::from(value))))
                    .or_else(|| any.downcast_ref::<i16>().copied().map(|value| int_value(i64::from(value))))
                    .or_else(|| any.downcast_ref::<i8>().copied().map(|value| int_value(i64::from(value))))
                    .or_else(|| any.downcast_ref::<isize>().copied().and_then(|value| i64::try_from(value).ok()).map(int_value)),
                Some(DaedalusTypeExpr::Scalar(DaedalusValueType::String)) => {
                    any.downcast_ref::<String>().cloned().map(string_value).or_else(|| any.downcast_ref::<Arc<String>>().map(|raw| string_value((**raw).clone())))
                }
                Some(DaedalusTypeExpr::Scalar(DaedalusValueType::Bytes)) => {
                    any.downcast_ref::<Vec<u8>>().cloned().map(bytes_value).or_else(|| any.downcast_ref::<Arc<[u8]>>().map(|bytes| bytes_value(bytes.as_ref().to_vec())))
                }
                _ => {
                    if let Some(value) = any.downcast_ref::<bool>().copied() {
                        return Some(DaedalusValue::Bool(value));
                    }
                    if let Some(value) = any.downcast_ref::<i64>().copied() {
                        return Some(int_value(value));
                    }
                    if let Some(value) = any.downcast_ref::<f64>().copied() {
                        return float_value(value);
                    }
                    if let Some(value) = any.downcast_ref::<String>() {
                        return Some(string_value(value.clone()));
                    }
                    None
                }
            }
        }
        DaedalusEdgePayload::Unit => None,
        _ => None,
    }
}
