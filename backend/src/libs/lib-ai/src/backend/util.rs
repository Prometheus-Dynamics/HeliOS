use crate::{
    error::{AiError, Result},
    model::ModelSource,
    tensor::{Tensor, TensorElementType, TensorShape},
};
use std::{convert::TryInto, fs};
use tract_core::{internal::Tensor as TractTensor, prelude::*};

const EDGE_TPU_SIGNATURE: &[u8] = b"edgetpu-custom-op";

fn element_count(shape: &TensorShape) -> usize {
    shape.iter().product()
}

pub fn read_model_bytes(source: &ModelSource) -> Result<Vec<u8>> {
    match source {
        ModelSource::Bytes(bytes) => Ok(bytes.clone()),
        ModelSource::File(path) => fs::read(path).map_err(|err| AiError::ModelLoadFailed { reason: format!("unable to read model at {}: {err}", path.display()) }),
        ModelSource::RegistryReference(reference) => Err(AiError::unsupported(format!("registry-based model references are not supported yet (requested {reference})"))),
    }
}

pub fn tensor_into_tract(tensor: Tensor) -> Result<TractTensor> {
    let name = tensor.name.clone();
    let shape = tensor.shape.clone();
    let bytes = tensor.bytes.ok_or_else(|| AiError::InvalidInput { reason: format!("tensor {name} did not contain any data") })?;

    let expected_elems = element_count(&shape);

    macro_rules! ensure_len {
        ($len: expr, $elem_size: expr) => {
            if bytes.len() != $len * $elem_size {
                return Err(AiError::InvalidInput { reason: format!("tensor {name} expected {} bytes for {:?} but received {}", $len * $elem_size, tensor.element_type, bytes.len()) });
            }
        };
    }

    let tensor = match tensor.element_type {
        TensorElementType::F32 => {
            ensure_len!(expected_elems, std::mem::size_of::<f32>());
            let mut values = Vec::<f32>::with_capacity(expected_elems);
            for chunk in bytes.chunks_exact(4) {
                values.push(f32::from_le_bytes(chunk.try_into().unwrap()));
            }
            TractTensor::from_shape(&shape, &values).map_err(|err| AiError::InvalidInput { reason: format!("tensor {name} shape mismatch: {err}") })?
        }
        TensorElementType::I8 => {
            ensure_len!(expected_elems, std::mem::size_of::<i8>());
            let values: Vec<i8> = bytes.into_iter().map(|b| b as i8).collect();
            TractTensor::from_shape(&shape, &values).map_err(|err| AiError::InvalidInput { reason: format!("tensor {name} shape mismatch: {err}") })?
        }
        TensorElementType::U8 => {
            ensure_len!(expected_elems, std::mem::size_of::<u8>());
            TractTensor::from_shape(&shape, &bytes).map_err(|err| AiError::InvalidInput { reason: format!("tensor {name} shape mismatch: {err}") })?
        }
        other => {
            return Err(AiError::unsupported(format!("conversion to tract tensor not implemented for {other:?}")));
        }
    };

    Ok(tensor)
}

pub fn is_edge_tpu_compiled_model(bytes: &[u8]) -> bool {
    bytes.windows(EDGE_TPU_SIGNATURE.len()).any(|window| window == EDGE_TPU_SIGNATURE)
}

pub fn guard_edge_tpu_support(bytes: &[u8]) -> Result<()> {
    if is_edge_tpu_compiled_model(bytes) {
        return Err(AiError::unsupported("Edge TPU-compiled TensorFlow Lite models require the Coral backend. Choose a CPU-compatible .tflite or set device_path to Coral."));
    }
    Ok(())
}

pub fn tract_tensor_into_tensor(name: Option<&str>, tensor: TractTensor) -> Result<Tensor> {
    let element_type = match tensor.datum_type() {
        DatumType::F32 => TensorElementType::F32,
        DatumType::I8 => TensorElementType::I8,
        DatumType::U8 => TensorElementType::U8,
        dt => {
            return Err(AiError::unsupported(format!("conversion from tract tensor for datum type {dt:?} is not implemented")));
        }
    };

    let shape: TensorShape = tensor.shape().to_vec();
    let bytes = match element_type {
        TensorElementType::F32 => {
            let array = tensor.to_array_view::<f32>().map_err(|err| AiError::InferenceFailed { reason: err.to_string() })?.to_owned();
            let (values, _) = array.into_raw_vec_and_offset();
            let mut bytes = Vec::with_capacity(values.len() * std::mem::size_of::<f32>());
            for value in values {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
            bytes
        }
        TensorElementType::I8 => {
            let array = tensor.to_array_view::<i8>().map_err(|err| AiError::InferenceFailed { reason: err.to_string() })?.to_owned();
            let (values, _) = array.into_raw_vec_and_offset();
            values.into_iter().map(|v| v as u8).collect()
        }
        TensorElementType::U8 => tensor.to_array_view::<u8>().map_err(|err| AiError::InferenceFailed { reason: err.to_string() })?.to_owned().into_raw_vec_and_offset().0,
        _ => unreachable!(),
    };

    Ok(Tensor::new(name.unwrap_or_default(), element_type, shape).with_bytes(bytes))
}
