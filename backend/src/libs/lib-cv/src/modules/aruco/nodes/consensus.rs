use super::*;

#[derive(Clone, Debug, NodeConfig)]
struct ArucoTagConsensusDetectionsConfig {
    #[port(default = 2i64, meta(ui_min = 1, ui_max = 10, ui_step = 1))]
    min_sources: i64,
    #[port(default = 10.0f64, meta(ui_min = 0.0, ui_max = 50.0, ui_step = 1.0))]
    center_dist_px: f64,
    #[port(default = -1i64, meta(ui_min = -1, ui_max = 10000, ui_step = 1))]
    min_id: i64,
    #[port(default = -1i64, meta(ui_min = -1, ui_max = 10000, ui_step = 1))]
    max_id: i64,
}

#[node(
    id = "consensus_detections",
    inputs(
        port(name = "sources", source = "ArucoDetections2D", ty = crate::daedalus_types::aruco_detections_2d()),
        config = ArucoTagConsensusDetectionsConfig
    ),
    outputs(port(name = "detections", source = "ArucoDetections2D", ty = crate::daedalus_types::aruco_detections_2d()))
)]
fn cv_aruco_consensus_detections(sources: FanIn<Vec<ArucoDetection2D>>, cfg: ArucoTagConsensusDetectionsConfig) -> Result<Vec<ArucoDetection2D>, NodeError> {
    fn center(det: &ArucoDetection2D) -> (f64, f64) {
        let mut sx = 0.0f64;
        let mut sy = 0.0f64;
        for p in det.corners {
            sx += p.x;
            sy += p.y;
        }
        (sx * 0.25, sy * 0.25)
    }

    fn area(det: &ArucoDetection2D) -> f64 {
        let pts = det.corners;
        let mut sum = 0.0f64;
        for i in 0..4 {
            let j = (i + 1) & 3;
            sum += pts[i].x * pts[j].y - pts[j].x * pts[i].y;
        }
        0.5 * sum.abs()
    }

    #[derive(Clone)]
    struct Cluster {
        id: u32,
        cx: f64,
        cy: f64,
        count: usize,
        best_area: f64,
        best: ArucoDetection2D,
    }

    let mut sources = sources.into_vec();
    if sources.is_empty() {
        return Ok(Vec::new());
    }
    let min_sources = cfg.min_sources.clamp(1, sources.len().max(1) as i64) as usize;
    let center_dist = cfg.center_dist_px.max(0.0);
    let center_dist_px2 = center_dist * center_dist;

    for dets in &mut sources {
        *dets = filter_detections_id_range(dets.as_slice(), cfg.min_id, cfg.max_id);
    }

    let mut clusters: Vec<Cluster> = Vec::new();
    for dets in sources {
        for det in dets {
            let (cx, cy) = center(&det);
            let a = area(&det);
            let mut matched = false;
            for cluster in &mut clusters {
                if cluster.id != det.id {
                    continue;
                }
                let dx = cluster.cx - cx;
                let dy = cluster.cy - cy;
                if dx * dx + dy * dy > center_dist_px2 {
                    continue;
                }
                cluster.count += 1;
                if a > cluster.best_area {
                    cluster.best_area = a;
                    cluster.best = det.clone();
                    cluster.cx = cx;
                    cluster.cy = cy;
                }
                matched = true;
                break;
            }
            if !matched {
                clusters.push(Cluster { id: det.id, cx, cy, count: 1, best_area: a, best: det });
            }
        }
    }

    Ok(clusters.into_iter().filter(|c| c.count >= min_sources).map(|c| c.best).collect())
}
