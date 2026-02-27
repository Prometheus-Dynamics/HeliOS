#[derive(Debug, Clone)]
pub struct PhotonPipelineMetadata {
    pub sequence_id: i64,
    pub capture_timestamp_micros: i64,
    pub publish_timestamp_micros: i64,
}

#[derive(Debug, Clone)]
pub struct Transform3d {
    pub translation: (f64, f64, f64),
}

#[derive(Debug, Clone)]
pub struct PhotonTrackedTarget {
    pub yaw: f64,
    pub pitch: f64,
    pub area: f64,
    pub skew: f64,
    pub fiducial_id: i32,
    pub best_camera_to_target: Transform3d,
    pub pose_ambiguity: f64,
}

#[derive(Debug, Clone)]
pub struct PhotonPipelineResult {
    pub metadata: PhotonPipelineMetadata,
    pub targets: Vec<PhotonTrackedTarget>,
}

pub fn decode_photon_pipeline_result(bytes: &[u8]) -> Result<PhotonPipelineResult, String> {
    let mut packet = PacketReader::new(bytes);
    let metadata = decode_metadata(&mut packet)?;
    let targets = decode_list(&mut packet, decode_tracked_target)?;
    skip_optional(&mut packet, skip_multitag_result)?;
    Ok(PhotonPipelineResult { metadata, targets })
}

struct PacketReader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> PacketReader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    fn read_exact(&mut self, len: usize) -> Result<&'a [u8], String> {
        let end = self.pos.checked_add(len).ok_or_else(|| "packet overflow".to_string())?;
        if end > self.data.len() {
            return Err("packet too short".to_string());
        }
        let slice = &self.data[self.pos..end];
        self.pos = end;
        Ok(slice)
    }

    fn read_u8(&mut self) -> Result<u8, String> {
        Ok(self.read_exact(1)?[0])
    }

    fn read_bool(&mut self) -> Result<bool, String> {
        Ok(self.read_u8()? == 1)
    }

    fn read_i16_le(&mut self) -> Result<i16, String> {
        let b = self.read_exact(2)?;
        Ok(i16::from_le_bytes([b[0], b[1]]))
    }

    fn read_i32_le(&mut self) -> Result<i32, String> {
        let b = self.read_exact(4)?;
        Ok(i32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    fn read_i64_le(&mut self) -> Result<i64, String> {
        let b = self.read_exact(8)?;
        Ok(i64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]]))
    }

    fn read_f32_le(&mut self) -> Result<f32, String> {
        let b = self.read_exact(4)?;
        Ok(f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    fn read_f64_le(&mut self) -> Result<f64, String> {
        let b = self.read_exact(8)?;
        Ok(f64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]]))
    }
}

fn decode_metadata(packet: &mut PacketReader<'_>) -> Result<PhotonPipelineMetadata, String> {
    let sequence_id = packet.read_i64_le()?;
    let capture_timestamp_micros = packet.read_i64_le()?;
    let publish_timestamp_micros = packet.read_i64_le()?;
    let _ = packet.read_i64_le()?; // time_since_last_pong
    Ok(PhotonPipelineMetadata { sequence_id, capture_timestamp_micros, publish_timestamp_micros })
}

fn decode_transform3d(packet: &mut PacketReader<'_>) -> Result<Transform3d, String> {
    let x = packet.read_f64_le()?;
    let y = packet.read_f64_le()?;
    let z = packet.read_f64_le()?;

    // rotation quaternion wxyz
    let _ = packet.read_f64_le()?;
    let _ = packet.read_f64_le()?;
    let _ = packet.read_f64_le()?;
    let _ = packet.read_f64_le()?;

    Ok(Transform3d { translation: (x, y, z) })
}

fn decode_tracked_target(packet: &mut PacketReader<'_>) -> Result<PhotonTrackedTarget, String> {
    let yaw = packet.read_f64_le()?;
    let pitch = packet.read_f64_le()?;
    let area = packet.read_f64_le()?;
    let skew = packet.read_f64_le()?;
    let fiducial_id = packet.read_i32_le()?;
    // objDetectId + objDetectConf
    let _ = packet.read_i32_le()?;
    let _ = packet.read_f32_le()?;
    let best_camera_to_target = decode_transform3d(packet)?;
    // altCameraToTarget
    let _ = decode_transform3d(packet)?;
    let pose_ambiguity = packet.read_f64_le()?;
    // minAreaRectCorners + detectedCorners
    skip_corner_list(packet)?;
    skip_corner_list(packet)?;

    Ok(PhotonTrackedTarget { yaw, pitch, area, skew, fiducial_id, best_camera_to_target, pose_ambiguity })
}

fn decode_list<T>(packet: &mut PacketReader<'_>, mut decode_item: impl FnMut(&mut PacketReader<'_>) -> Result<T, String>) -> Result<Vec<T>, String> {
    let len = packet.read_u8()? as usize;
    let mut out = Vec::with_capacity(len);
    for _ in 0..len {
        out.push(decode_item(packet)?);
    }
    Ok(out)
}

fn skip_short_list(packet: &mut PacketReader<'_>) -> Result<(), String> {
    let len = packet.read_u8()? as usize;
    for _ in 0..len {
        let _ = packet.read_i16_le()?;
    }
    Ok(())
}

fn skip_optional(packet: &mut PacketReader<'_>, skip_item: impl FnOnce(&mut PacketReader<'_>) -> Result<(), String>) -> Result<(), String> {
    let present = packet.read_bool()?;
    if present {
        skip_item(packet)?;
    }
    Ok(())
}

fn skip_corner_list(packet: &mut PacketReader<'_>) -> Result<(), String> {
    let len = packet.read_u8()? as usize;
    for _ in 0..len {
        let _ = packet.read_f64_le()?;
        let _ = packet.read_f64_le()?;
    }
    Ok(())
}

fn skip_multitag_result(packet: &mut PacketReader<'_>) -> Result<(), String> {
    // PnpResult: best Transform3d, alt Transform3d, bestReprojErr, altReprojErr, ambiguity
    let _ = decode_transform3d(packet)?;
    let _ = decode_transform3d(packet)?;
    let _ = packet.read_f64_le()?;
    let _ = packet.read_f64_le()?;
    let _ = packet.read_f64_le()?;

    // fiducialIDsUsed[?] (int16 list)
    skip_short_list(packet)?;
    Ok(())
}
