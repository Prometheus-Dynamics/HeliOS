use std::sync::Arc;

use crate::http::peers::{PeerInfo, PeerRemoteStreamsResponse, PeersServiceState};

#[derive(Clone, Default)]
pub struct NetworkService {
    state: Arc<crate::http::device::network::DeviceNetworkState>,
}

impl NetworkService {
    pub(crate) fn inner(&self) -> &crate::http::device::network::DeviceNetworkState {
        self.state.as_ref()
    }

    pub fn spawn_team_autodetect_task(&self) {
        self.state.clone().spawn_team_autodetect_task();
    }
}

#[derive(Clone, Default)]
pub struct PeersService {
    state: Arc<PeersServiceState>,
}

impl PeersService {
    pub(crate) fn inner(&self) -> &PeersServiceState {
        self.state.as_ref()
    }

    pub async fn init_from_disk(&self) {
        self.state.init_from_disk().await;
    }

    pub async fn snapshot_peers(&self) -> Vec<PeerInfo> {
        self.state.snapshot_peers().await
    }

    pub async fn snapshot_peer_streams(&self, state: &crate::http::AppState) -> PeerRemoteStreamsResponse {
        self.state.snapshot_peer_streams(state).await
    }
}
