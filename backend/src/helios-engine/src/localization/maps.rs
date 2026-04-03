use lib_cv::modules::aruco::tag::ArucoTagDecoding;
use lib_schema_migration::{normalize_to_current, SyncSchemaPlan};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

pub const CURRENT_FIELD_MAP_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct FieldMapSummary {
    pub id: String,
    pub name: String,
    pub width_m: f64,
    pub depth_m: f64,
    pub marker_count: usize,
    pub source_kind: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct FieldMapDocument {
    pub schema_version: u32,
    pub id: String,
    pub name: String,
    pub width_m: f64,
    pub depth_m: f64,
    pub markers: Vec<FieldMapMarker>,
    pub source: FieldMapSource,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub overlay: Option<FieldMapOverlay>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct FieldMapOverlay {
    pub data_url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub opacity: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width_m: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub depth_m: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset_x_m: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset_z_m: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rotation_deg: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct FieldMapMarker {
    pub id: u32,
    pub family: String,
    pub size_m: f64,
    pub position: [f64; 3],
    pub quaternion: FieldQuaternion,
    #[serde(default)]
    pub heading_deg: f64,
    #[serde(default)]
    pub tag_bits: Option<FieldMapTagBits>,
    #[serde(default)]
    pub unique: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct FieldMapTagBits {
    pub width: u8,
    pub border: u8,
    pub rows: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct FieldQuaternion {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub w: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum FieldMapSource {
    LimelightFmap {
        #[serde(default)]
        original_file_name: Option<String>,
        #[serde(default)]
        map_type: Option<String>,
    },
}

const FIELD_MAP_SCHEMA_PLAN: SyncSchemaPlan<serde_json::Value> = SyncSchemaPlan::strict("field map document", CURRENT_FIELD_MAP_SCHEMA_VERSION);

pub fn parse_field_map_document(bytes: &[u8]) -> Result<(FieldMapDocument, bool), String> {
    let raw = serde_json::from_slice::<serde_json::Value>(bytes).map_err(|err| format!("failed to decode field map document: {err}"))?;
    let migrated = normalize_to_current(raw.clone(), &FIELD_MAP_SCHEMA_PLAN)?;
    let parsed = serde_json::from_value::<FieldMapDocument>(migrated.clone()).map_err(|err| format!("failed to parse field map document: {err}"))?;
    Ok((parsed, migrated != raw))
}

pub fn hydrate_map_document(doc: &mut FieldMapDocument) {
    for marker in &mut doc.markers {
        if marker.tag_bits.is_none() {
            marker.tag_bits = aruco_bits_for_family(&marker.family, marker.id);
        }
        if marker.heading_deg == 0.0 && !(marker.quaternion.x == 0.0 && marker.quaternion.y == 0.0 && marker.quaternion.z == 0.0 && marker.quaternion.w == 1.0) {
            marker.heading_deg = heading_from_quaternion(&marker.quaternion);
        }
    }
}

fn heading_from_quaternion(quaternion: &FieldQuaternion) -> f64 {
    use nalgebra::{Quaternion, UnitQuaternion, Vector3};
    let quat = UnitQuaternion::from_quaternion(Quaternion::new(quaternion.w, quaternion.x, quaternion.y, quaternion.z));
    let normal = quat.transform_vector(&Vector3::new(1.0, 0.0, 0.0));
    normal.x.atan2(normal.z).to_degrees()
}

pub fn aruco_bits_for_family(family_label: &str, id: u32) -> Option<FieldMapTagBits> {
    let normalized = normalize_aruco_family_label(family_label)?;
    let family = lib_cv::modules::aruco::tag::ArucoTagFamily::from_label(normalized)?;
    let candidates = [Some(id), id.checked_sub(1)];
    for candidate in candidates.into_iter().flatten() {
        if let Some(grid) = family.bit_grid(candidate as usize) {
            return Some(FieldMapTagBits { width: grid.width, border: grid.border, rows: grid.rows });
        }
    }
    None
}

fn normalize_aruco_family_label(raw: &str) -> Option<&'static str> {
    let value = raw.trim().to_ascii_lowercase();
    let direct = lib_cv::modules::aruco::tag::ArucoTagFamily::available().iter().find(|&&label| value == label).copied();
    if direct.is_some() {
        return direct;
    }

    // Limelight/WPILib family strings often embed the code family, e.g. "apriltag3_36h11_classic".
    // Accept common aliases by substring match.
    lib_cv::modules::aruco::tag::ArucoTagFamily::available().iter().find(|&&label| value.contains(label)).copied()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn limelight_family_strings_produce_bits() {
        let bits = aruco_bits_for_family("36h11", 1);
        assert!(bits.is_some());
        let bits = bits.unwrap();
        assert!(bits.width > 0);
        assert_eq!(bits.rows.len(), bits.width as usize);
        assert!(bits.rows.iter().all(|row| row.len() == bits.width as usize));

        let bits = aruco_bits_for_family("apriltag3_36h11_classic", 1);
        assert!(bits.is_some());

        // FRC tags are typically indexed as 1..N; ensure common ids resolve.
        assert!(aruco_bits_for_family("apriltag3_36h11_classic", 15).is_some());
        assert!(aruco_bits_for_family("apriltag3_36h11_classic", 16).is_some());
    }

    #[test]
    fn parse_field_map_document_rejects_missing_schema_version() {
        let bytes = serde_json::to_vec(&json!({
            "id": "field-a",
            "name": "Field A",
            "width_m": 1.0,
            "depth_m": 2.0,
            "markers": [],
            "source": { "kind": "limelight_fmap" }
        }))
        .expect("encode field map");
        let err = parse_field_map_document(&bytes).expect_err("missing schema version should fail");
        assert!(err.contains("missing required schema_version"));
    }

    #[test]
    fn parse_field_map_document_rejects_future_schema_version() {
        let bytes = serde_json::to_vec(&json!({
            "schema_version": CURRENT_FIELD_MAP_SCHEMA_VERSION + 1,
            "id": "field-a",
            "name": "Field A",
            "width_m": 1.0,
            "depth_m": 2.0,
            "markers": [],
            "source": { "kind": "limelight_fmap" }
        }))
        .expect("encode field map");
        let err = parse_field_map_document(&bytes).expect_err("future field map should fail");
        assert!(err.contains("unsupported field map document schema_version"));
    }
}
