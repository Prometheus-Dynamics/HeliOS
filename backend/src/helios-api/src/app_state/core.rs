use std::ops::Deref;
use std::sync::Arc;

use crate::ipc::IpcHandles;

use super::hardware_media::{HardwareReadModelService, MediaReadModelService};
use super::network_peers::{NetworkService, PeersService};
use super::pipelines_streams::{PipelinesReadModelService, StreamsReadModelService};
use super::runtime_coordination::RuntimeCoordinationService;
use super::system_updater::{SystemReadModelService, UpdaterService};

#[derive(Default)]
pub struct ApiServices {
    pub hardware: HardwareReadModelService,
    pub media: MediaReadModelService,
    pub network: NetworkService,
    pub peers: PeersService,
    pub pipelines: PipelinesReadModelService,
    pub runtime: RuntimeCoordinationService,
    pub streams: StreamsReadModelService,
    pub system: SystemReadModelService,
    pub updater: UpdaterService,
}

pub struct ApiAppState {
    ipc: Arc<IpcHandles>,
    pub services: ApiServices,
}

impl ApiAppState {
    pub fn new(ipc: Arc<IpcHandles>) -> Self {
        Self { ipc, services: ApiServices::default() }
    }

    pub fn ipc(&self) -> &Arc<IpcHandles> {
        &self.ipc
    }
}

impl Deref for ApiAppState {
    type Target = IpcHandles;

    fn deref(&self) -> &Self::Target {
        self.ipc.as_ref()
    }
}
