use uuid::Uuid;

/// Shared runtime-visible identifiers that must stay stable across engine, API,
/// persistence, and generated frontend contracts.
pub mod stream_ids {
    use super::Uuid;

    /// Reserved system pipeline that represents the built-in raw stream graph.
    pub const RAW_PIPELINE_UUID: Uuid = Uuid::from_u128(0x00000000_0000_0000_0000_0000000000aa);

    /// Historical reserved raw pipeline id used in older persisted manifests.
    pub const LEGACY_RAW_PIPELINE_UUID: Uuid = Uuid::from_u128(0x00000000_0000_0000_0000_0000000000ab);

    /// Reserved internal pipeline used for transient calibration-mode graphs.
    pub const CALIBRATION_MODE_PIPELINE_UUID: Uuid = Uuid::from_u128(0x00000000_0000_0000_0000_00000000c411);
}

pub mod localization {
    /// Reserved synthetic external-source id that exposes the device IMU.
    pub const DEVICE_IMU_EXTERNAL_SOURCE_ID: &str = "imu";
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
