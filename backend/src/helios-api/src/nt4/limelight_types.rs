use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use utoipa::ToSchema;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub struct LimelightAdapterId {
    pub stream_id: Uuid,
    pub table_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub struct LimelightReadSnapshot {
    pub tv: f64,
    pub tx: f64,
    pub ty: f64,
    pub tid: f64,
    #[serde(default)]
    pub t2d: Vec<f64>,
    #[serde(default)]
    pub botpose: Vec<f64>,
    #[serde(default)]
    pub botpose_wpiblue: Vec<f64>,
    #[serde(default)]
    pub botpose_wpired: Vec<f64>,
    #[serde(default)]
    pub rawfiducials: Vec<f64>,
    #[serde(default)]
    pub rawdetections: Vec<f64>,
    #[serde(default)]
    pub rawtargets: Vec<f64>,
    #[serde(default)]
    pub tcornxy: Vec<f64>,
    pub tl: f64,
    pub cl: f64,
    pub hb: f64,
    #[serde(default)]
    pub hw: Vec<f64>,
    #[serde(default)]
    pub imu: Vec<f64>,
    #[serde(default)]
    pub json: String,
}

impl Default for LimelightReadSnapshot {
    fn default() -> Self {
        Self {
            tv: 0.0,
            tx: 0.0,
            ty: 0.0,
            tid: -1.0,
            t2d: Vec::new(),
            botpose: Vec::new(),
            botpose_wpiblue: Vec::new(),
            botpose_wpired: Vec::new(),
            rawfiducials: Vec::new(),
            rawdetections: Vec::new(),
            rawtargets: Vec::new(),
            tcornxy: Vec::new(),
            tl: 0.0,
            cl: 0.0,
            hb: 0.0,
            hw: vec![0.0, 0.0, 0.0, 0.0],
            imu: vec![0.0; 10],
            json: "{}".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, Default)]
#[serde(rename_all = "snake_case")]
pub struct LimelightControlState {
    #[serde(default)]
    pub pipeline: Option<f64>,
    #[serde(default)]
    pub led_mode: Option<f64>,
    #[serde(default)]
    pub stream_mode: Option<f64>,
    #[serde(default)]
    pub snapshot_counter: f64,
    #[serde(default)]
    pub crop: Option<[f64; 4]>,
    #[serde(default)]
    pub keystone_set: Option<[f64; 2]>,
    #[serde(default)]
    pub throttle_set: Option<f64>,
    #[serde(default)]
    pub priority_id: Option<f64>,
}

#[derive(Debug, Clone, Default)]
pub struct LimelightPublishCache {
    pub by_topic: BTreeMap<String, serde_json::Value>,
}
