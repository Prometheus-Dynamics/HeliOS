mod generated_runtime_contracts;

/// Shared runtime-visible identifiers that must stay stable across engine, API,
/// persistence, and generated frontend contracts.
pub mod stream_ids {
    pub use super::generated_runtime_contracts::stream_ids::{CALIBRATION_MODE_PIPELINE_UUID, LEGACY_RAW_PIPELINE_UUID, RAW_PIPELINE_UUID};
}

pub mod localization {
    pub use super::generated_runtime_contracts::localization::DEVICE_IMU_EXTERNAL_SOURCE_ID;
}

#[cfg(test)]
mod tests {
    use super::localization::DEVICE_IMU_EXTERNAL_SOURCE_ID;
    use super::stream_ids::{CALIBRATION_MODE_PIPELINE_UUID, LEGACY_RAW_PIPELINE_UUID, RAW_PIPELINE_UUID};

    #[test]
    fn reserved_stream_ids_are_stable_and_distinct() {
        assert_ne!(RAW_PIPELINE_UUID, LEGACY_RAW_PIPELINE_UUID);
        assert_ne!(RAW_PIPELINE_UUID, CALIBRATION_MODE_PIPELINE_UUID);
        assert_ne!(LEGACY_RAW_PIPELINE_UUID, CALIBRATION_MODE_PIPELINE_UUID);
    }

    #[test]
    fn device_imu_external_source_id_is_stable() {
        assert_eq!(DEVICE_IMU_EXTERNAL_SOURCE_ID, "imu");
    }
}
