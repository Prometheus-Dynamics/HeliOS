use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::Path;

use crate::math::{
    rotation::{Rotation2, Rotation3},
    translation::{Translation2, Translation3},
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MarkerPose {
    pub id: u32,
    pub translation: Translation3,
    pub rotation: Rotation3,
}

impl MarkerPose {
    pub fn new(id: u32, translation: Translation3, rotation: Rotation3) -> Self {
        Self { id, translation, rotation }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ArucoMap {
    pub markers: Vec<MarkerPose>,
}

impl ArucoMap {
    /// Parse an `ArucoMap` from a JSON string.
    pub fn from_json_str(s: &str) -> serde_json::Result<Self> {
        serde_json::from_str(s)
    }

    /// Serialize the map to a pretty JSON string.
    pub fn to_json_string(&self) -> serde_json::Result<String> {
        serde_json::to_string_pretty(self)
    }

    /// Load an `ArucoMap` from a JSON file.
    pub fn from_json_file(path: &Path) -> std::io::Result<Self> {
        let data = std::fs::read_to_string(path)?;
        serde_json::from_str(&data).map_err(std::io::Error::other)
    }

    /// Save the map to a JSON file.
    pub fn to_json_file(&self, path: &Path) -> std::io::Result<()> {
        let data = self.to_json_string().map_err(std::io::Error::other)?;
        std::fs::write(path, data)
    }

    /// Calculate a simple 2D pose averaged over the provided marker ids.
    pub fn calculate_2d_pose(&self, ids: &[u32]) -> Option<(Translation2, Rotation2)> {
        if ids.is_empty() {
            return None;
        }
        let id_set: HashSet<u32> = ids.iter().copied().collect();
        let mut count = 0f64;
        let mut sum_t = Translation2::default();
        let mut sum_r = Rotation2::default();
        for m in &self.markers {
            if id_set.contains(&m.id) {
                sum_t.x += m.translation.x;
                sum_t.y += m.translation.y;
                sum_r.yaw += m.rotation.yaw;
                count += 1.0;
            }
        }
        if count == 0.0 {
            return None;
        }
        Some((Translation2 { x: sum_t.x / count, y: sum_t.y / count }, Rotation2 { yaw: sum_r.yaw / count }))
    }

    /// Calculate a weighted 2D pose (x, y, yaw) using per-marker weights.
    pub fn calculate_weighted_2d_pose(&self, ids: &[u32], weights: &[f32]) -> Option<(Translation2, Rotation2)> {
        if ids.len() != weights.len() || ids.is_empty() {
            return None;
        }
        let mut sum_t = Translation2::default();
        let mut sum_r = Rotation2::default();
        let mut weight_sum = 0f64;
        for (&id, &w) in ids.iter().zip(weights) {
            if w <= 0.0 {
                continue;
            }
            if let Some(m) = self.get_marker(id) {
                let w = w as f64;
                sum_t.x += m.translation.x * w;
                sum_t.y += m.translation.y * w;
                sum_r.yaw += m.rotation.yaw * w;
                weight_sum += w;
            }
        }
        if weight_sum == 0.0 {
            return None;
        }
        Some((Translation2 { x: sum_t.x / weight_sum, y: sum_t.y / weight_sum }, Rotation2 { yaw: sum_r.yaw / weight_sum }))
    }

    /// Calculate a 3D pose (translation and rotation) averaged over the provided marker ids.
    pub fn calculate_3d_pose(&self, ids: &[u32]) -> Option<(Translation3, Rotation3)> {
        if ids.is_empty() {
            return None;
        }
        let id_set: HashSet<u32> = ids.iter().copied().collect();
        let mut count = 0f64;
        let mut t = Translation3::default();
        let mut r = Rotation3::default();
        for m in &self.markers {
            if id_set.contains(&m.id) {
                t.x += m.translation.x;
                t.y += m.translation.y;
                t.z += m.translation.z;
                r.roll += m.rotation.roll;
                r.pitch += m.rotation.pitch;
                r.yaw += m.rotation.yaw;
                count += 1.0;
            }
        }
        if count == 0.0 {
            return None;
        }
        Some((Translation3 { x: t.x / count, y: t.y / count, z: t.z / count }, Rotation3 { roll: r.roll / count, pitch: r.pitch / count, yaw: r.yaw / count }))
    }

    /// Calculate a weighted 3D pose using per-marker weights.
    pub fn calculate_weighted_3d_pose(&self, ids: &[u32], weights: &[f32]) -> Option<(Translation3, Rotation3)> {
        if ids.len() != weights.len() || ids.is_empty() {
            return None;
        }
        let mut t = Translation3::default();
        let mut r = Rotation3::default();
        let mut weight_sum = 0f64;
        for (&id, &w) in ids.iter().zip(weights) {
            if w <= 0.0 {
                continue;
            }
            if let Some(m) = self.get_marker(id) {
                let w = w as f64;
                t.x += m.translation.x * w;
                t.y += m.translation.y * w;
                t.z += m.translation.z * w;
                r.roll += m.rotation.roll * w;
                r.pitch += m.rotation.pitch * w;
                r.yaw += m.rotation.yaw * w;
                weight_sum += w;
            }
        }
        if weight_sum == 0.0 {
            return None;
        }
        Some((Translation3 { x: t.x / weight_sum, y: t.y / weight_sum, z: t.z / weight_sum }, Rotation3 { roll: r.roll / weight_sum, pitch: r.pitch / weight_sum, yaw: r.yaw / weight_sum }))
    }

    /// Add or update a marker pose.
    pub fn add_marker(&mut self, pose: MarkerPose) {
        if let Some(existing) = self.markers.iter_mut().find(|m| m.id == pose.id) {
            *existing = pose;
        } else {
            self.markers.push(pose);
        }
    }

    /// Remove a marker by id and return it if present.
    pub fn remove_marker(&mut self, id: u32) -> Option<MarkerPose> {
        if let Some(idx) = self.markers.iter().position(|m| m.id == id) { Some(self.markers.remove(idx)) } else { None }
    }

    /// Get a marker pose by id.
    pub fn get_marker(&self, id: u32) -> Option<&MarkerPose> {
        self.markers.iter().find(|m| m.id == id)
    }

    /// Number of markers in the map.
    pub fn len(&self) -> usize {
        self.markers.len()
    }

    /// Whether the map contains any markers.
    pub fn is_empty(&self) -> bool {
        self.markers.is_empty()
    }

    /// Merge another map into this one, overwriting poses with matching ids.
    pub fn merge(&mut self, other: &Self) {
        for m in &other.markers {
            self.add_marker(m.clone());
        }
    }
}
