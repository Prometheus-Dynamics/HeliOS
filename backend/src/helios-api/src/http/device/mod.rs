pub mod bootloader;
pub mod fan;
pub mod ide;
pub mod identity;
pub mod imu;
pub mod ipa;
pub mod lighting;
pub mod logs;
pub mod metrics;
pub mod network;
pub mod nt4;
pub mod os_release;
pub mod power;
pub mod resource_guard;
pub mod restart;
pub mod rig;
pub mod snapshots;
pub mod usb_power;

use axum::{Router, routing::get};

use super::AppState;

pub use imu::handle_imu_ws;
pub use metrics::{CpuCoreMetrics, DeviceMetrics, DiskMetrics, TempReading};
pub use power::{PowerSourcePayload, PowerStatusPayload};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/hostname", get(identity::hostname).post(identity::set_hostname))
        .route("/network", get(network::network).post(network::set_network))
        .route("/team", get(network::team).post(network::set_team))
        .route("/nt4", get(nt4::get_nt4_settings).post(nt4::set_nt4_settings))
        .route("/usb-power", get(usb_power::get_usb_power_settings).post(usb_power::set_usb_power_settings))
        .route("/fan/config", get(fan::fan_config).post(fan::update_fan_config))
        .route("/snapshots", get(snapshots::list_snapshots).post(snapshots::capture_snapshot))
        .route("/snapshots/:id", axum::routing::delete(snapshots::delete_snapshot))
        .route("/snapshots/:id/download", get(snapshots::download_snapshot))
        .route("/logs", get(logs::logs))
        .route("/logs/sources", get(logs::sources))
        .route("/logs/download", get(logs::download))
        .route("/console", get(logs::console))
        .route("/ipa", get(ipa::ipa_status))
        .route("/ipa/download", get(ipa::download))
        .route("/ipa/ccm", axum::routing::post(ipa::apply_ccm))
        .route("/ipa/ccm/solve", axum::routing::post(ipa::solve_ccm))
        .route("/ipa/ccm/chart", get(ipa::chart_png))
        .route("/ipa/ccm/chart.pdf", get(ipa::chart_pdf))
        .route("/ide", get(ide::ide_info))
        .route("/ide/projects", get(ide::ide_projects).post(ide::create_ide_project))
        .route("/i2c", get(imu::i2c))
        .route("/imu", get(imu::imu_status).patch(imu::update_imu))
        .route("/lighting", axum::routing::post(lighting::lighting_command))
        .route("/lighting/state", get(lighting::lighting_state))
        .route("/lighting/config", get(lighting::lighting_config).post(lighting::update_lighting_config))
        .route("/lighting/config/reset", axum::routing::post(lighting::reset_lighting_config))
        .route("/lighting/templates", get(lighting::list_lighting_templates))
        .route("/lighting/templates/:id", get(lighting::fetch_lighting_template))
        .route("/lighting/animations", get(lighting::list_lighting_animations).post(lighting::save_lighting_animation))
        .route("/lighting/animations/:name", axum::routing::delete(lighting::delete_lighting_animation))
        .route("/power", get(power::power_status))
        .route("/metrics", get(metrics::metrics))
        .route("/resource-guard", get(resource_guard::status))
        .route("/resource-guard/restore/:stream_id", axum::routing::post(resource_guard::restore))
        .route("/bootloader", get(bootloader::status).post(bootloader::update))
        .route("/os", get(os_release::os_release))
        .route("/restart", axum::routing::post(restart::restart))
        .merge(rig::router())
}
