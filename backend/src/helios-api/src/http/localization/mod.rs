use axum::Router;
use axum::routing::get;

use super::AppState;

pub mod config;
pub mod external;
pub mod maps;
mod media_imu;
pub mod peers;
pub mod pipeline;
pub mod solve;
pub mod sources;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/sources", get(sources::list_sources))
        .route("/config", get(config::get_config).put(config::update_config))
        .route("/solve", get(solve::solve))
        .route("/streams/:id/outputs/:output_key", get(sources::sample_output))
        .route("/peers/:id/outputs/:output_key", get(sources::sample_peer_output))
        .route("/profiles/:id/outputs/:output_key", get(sources::sample_profile_output))
        .merge(external::router())
        .merge(pipeline::router())
        .nest("/maps", maps::router())
}
