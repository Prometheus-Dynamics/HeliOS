use lib_sensors::model::json_from_sensor_reading;
use lib_sensors::model::{
    SensorDescriptorModel, descriptor_from_device as model_descriptor_from_device, descriptor_from_fan_status as model_descriptor_from_fan_status,
    descriptor_from_led_config as model_descriptor_from_led_config,
};
use lib_sensors::{fan_config, led_config, sensor_config};
use serde_json::Value as JsonValue;

use crate::dto::{JsonData, SensorDescriptor};

mod coral;
mod coral_diagnostics;
mod i2c;
mod led;

pub(crate) use coral::build_coral_descriptors;
pub(crate) use i2c::read_i2c_inventory;
pub(crate) use led::resolve_led_config;

fn to_descriptor(model: SensorDescriptorModel) -> SensorDescriptor {
    let value = model.value.as_ref().map(json_from_sensor_reading);
    SensorDescriptor {
        backend: model.backend,
        identifier: model.identifier,
        present: model.present,
        info: Some(JsonData::from_value(&JsonValue::Object(model.info))),
        metadata: Some(JsonData::from_value(&JsonValue::Object(model.metadata))),
        stream_id: model.stream_id,
        value: value.as_ref().map(JsonData::from_value),
    }
}

pub(crate) fn descriptor_from_device(device: &sensor_config::SensorDeviceCfg) -> SensorDescriptor {
    to_descriptor(model_descriptor_from_device(device))
}

pub(crate) fn descriptor_from_fan_status(status: &fan_config::FanStatus) -> SensorDescriptor {
    to_descriptor(model_descriptor_from_fan_status(status))
}

pub(crate) fn descriptor_from_led_config(config: &led_config::LedConfig) -> SensorDescriptor {
    to_descriptor(model_descriptor_from_led_config(config))
}
