mod binding;
mod sampling;
mod source;
#[cfg(test)]
mod tests;

pub(crate) use sampling::{fetch_media_imu_sample, fetch_media_imu_sample_for_stream};
pub(crate) use source::{is_media_imu_source_id, source_for_stream};

pub(crate) use binding::{MEDIA_IMU_OUTPUT_KEY, MEDIA_IMU_OUTPUT_KEY_LEGACY};
