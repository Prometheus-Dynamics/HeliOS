mod probe;
mod state;
mod streams;
mod types;

use axum::{
    Router,
    routing::{delete, get, post},
};

use super::AppState;

pub use types::*;

pub(crate) use probe::{
    __path_photonvision_discover_streams, __path_probe_peer, photonvision_discover_streams,
    probe_peer,
};
pub(crate) use state::{
    __path_discover_peers, __path_list_peers, __path_register_peer, __path_remove_peer,
    PeersServiceState, discover_peers, init_peers_from_disk, list_peers, register_peer,
    remove_peer, snapshot_peer_streams, snapshot_peers,
};
pub(crate) use streams::{
    list_peer_streams, list_peer_streams_for_peer, parse_peer_scoped_ref, peer_v1_url,
    proxy_peer_stream_format, proxy_peer_stream_frame, proxy_peer_stream_preview,
    sync_peer_pipelines,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list_peers).post(register_peer))
        .route("/streams", get(list_peer_streams))
        .route("/discover", post(discover_peers))
        .route("/probe", post(probe_peer))
        .route(
            "/integrations/photonvision/streams",
            post(photonvision_discover_streams),
        )
        .route("/{id}/streams", get(list_peer_streams_for_peer))
        .route("/{id}/streams/{stream_id}/format", get(proxy_peer_stream_format))
        .route(
            "/{id}/streams/{stream_id}/preview",
            get(proxy_peer_stream_preview),
        )
        .route("/{id}/streams/{stream_id}/frame", get(proxy_peer_stream_frame))
        .route("/{id}/pipelines/sync", post(sync_peer_pipelines))
        .route("/{id}", delete(remove_peer))
}
