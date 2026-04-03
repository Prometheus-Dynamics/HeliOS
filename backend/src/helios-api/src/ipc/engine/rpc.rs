use super::{
    EngineConnection, ExpectedEvent, StartRecordingParams,
    timeouts::{
        ENGINE_CAPTURE_SHADOW_TIMEOUT, ENGINE_PIPELINE_LAYOUT_TIMEOUT, ENGINE_RESPONSE_TIMEOUT, ENGINE_START_STREAM_TIMEOUT, ENGINE_STOP_RECORDING_TIMEOUT, ENGINE_STOP_STREAM_TIMEOUT,
        calibration_solve_timeout, localization_solve_timeout,
    },
};
use helios_engine::ipc::{
    CalibrationSolveRequest, EngineCommand, EngineEvent, JsonWire, LocalizationPipelineGraphRequest, LocalizationPipelineSampleRequest, LocalizationPipelineStatusRequest, LocalizationSolveRequest,
    NodeRegistrySnapshot, ResolvedStreamConfig, StreamCalibration, StreamRuntimeCapabilities,
};
use std::{collections::BTreeMap, io, time::Duration};
use tracing::warn;

impl EngineConnection {
    pub async fn start_stream(&self, manifest: ResolvedStreamConfig) -> Result<EngineEvent, lib_ipc::client::ClientTransportError> {
        self.request(|command_id| EngineCommand::Start { command_id, manifest: Box::new(manifest) }, ExpectedEvent::Start, "start_stream", ENGINE_START_STREAM_TIMEOUT).await
    }

    pub async fn stop_stream(&self, id: uuid::Uuid) -> Result<EngineEvent, lib_ipc::client::ClientTransportError> {
        self.stop_stream_with_timeout(id, ENGINE_STOP_STREAM_TIMEOUT).await
    }

    pub async fn stop_stream_with_timeout(&self, id: uuid::Uuid, timeout: Duration) -> Result<EngineEvent, lib_ipc::client::ClientTransportError> {
        self.request(|command_id| EngineCommand::Stop { command_id, stream_id: id }, ExpectedEvent::Stop { stream_id: id }, "stop_stream", timeout).await
    }

    pub async fn get_controls(&self, id: uuid::Uuid) -> Result<EngineEvent, lib_ipc::client::ClientTransportError> {
        self.request(|command_id| EngineCommand::GetControls { command_id, stream_id: id }, ExpectedEvent::Controls { stream_id: id }, "get_controls", ENGINE_RESPONSE_TIMEOUT).await
    }

    pub async fn get_metrics(&self, id: uuid::Uuid) -> Result<EngineEvent, lib_ipc::client::ClientTransportError> {
        self.request(|command_id| EngineCommand::GetMetrics { command_id, stream_id: id }, ExpectedEvent::Metrics { stream_id: id }, "get_metrics", ENGINE_RESPONSE_TIMEOUT).await
    }

    pub async fn snapshot_jpeg(&self, id: uuid::Uuid, quality: u8, source: Option<helios_engine::ipc::RecordingSource>) -> Result<Vec<u8>, lib_ipc::client::ClientTransportError> {
        match self
            .request(|command_id| EngineCommand::SnapshotJpeg { command_id, stream_id: id, quality, source }, ExpectedEvent::SnapshotJpeg { stream_id: id }, "snapshot_jpeg", ENGINE_RESPONSE_TIMEOUT)
            .await?
        {
            EngineEvent::SnapshotJpeg { bytes, .. } => Ok(bytes),
            EngineEvent::Nack { reason, .. } => Err(lib_ipc::client::ClientTransportError::Io(io::Error::other(reason))),
            other => {
                warn!(?other, "engine returned unexpected event for snapshot_jpeg after filtering");
                Err(lib_ipc::client::ClientTransportError::Io(io::Error::new(io::ErrorKind::InvalidData, "unexpected engine reply for snapshot_jpeg")))
            }
        }
    }

    pub async fn start_recording(&self, id: uuid::Uuid, params: StartRecordingParams) -> Result<EngineEvent, lib_ipc::client::ClientTransportError> {
        let StartRecordingParams { source, output_path, container, codec, duration_ms, settings } = params;
        self.request(
            |command_id| EngineCommand::StartRecording { command_id, stream_id: id, source, output_path, container, codec, duration_ms, settings },
            ExpectedEvent::Ack,
            "start_recording",
            ENGINE_RESPONSE_TIMEOUT,
        )
        .await
    }

    pub async fn stop_recording(&self, id: uuid::Uuid) -> Result<EngineEvent, lib_ipc::client::ClientTransportError> {
        self.request(|command_id| EngineCommand::StopRecording { command_id, stream_id: id }, ExpectedEvent::Ack, "stop_recording", ENGINE_STOP_RECORDING_TIMEOUT).await
    }

    pub async fn capture_shadow_recording(
        &self,
        id: uuid::Uuid,
        output_path: String,
        container: helios_engine::ipc::RecordingContainer,
        window_ms: u64,
    ) -> Result<EngineEvent, lib_ipc::client::ClientTransportError> {
        self.request(
            |command_id| EngineCommand::CaptureShadowRecording { command_id, stream_id: id, output_path, container, window_ms },
            ExpectedEvent::Ack,
            "capture_shadow_recording",
            ENGINE_CAPTURE_SHADOW_TIMEOUT,
        )
        .await
    }

    pub async fn get_node_registry(&self) -> Result<NodeRegistrySnapshot, lib_ipc::client::ClientTransportError> {
        self.get_node_registry_with_timeout(ENGINE_RESPONSE_TIMEOUT).await
    }

    pub async fn get_node_registry_with_timeout(&self, timeout: Duration) -> Result<NodeRegistrySnapshot, lib_ipc::client::ClientTransportError> {
        match self.request(|command_id| EngineCommand::GetNodeRegistry { command_id }, ExpectedEvent::NodeRegistry, "get_node_registry", timeout).await? {
            EngineEvent::NodeRegistry { snapshot, .. } => Ok(snapshot),
            EngineEvent::Nack { reason, .. } => Err(lib_ipc::client::ClientTransportError::Io(io::Error::other(reason))),
            other => {
                warn!(?other, "engine returned unexpected event for get_node_registry after filtering");
                Err(lib_ipc::client::ClientTransportError::Io(io::Error::new(io::ErrorKind::InvalidData, "unexpected engine reply for get_node_registry")))
            }
        }
    }

    pub async fn discover_devices(&self) -> Result<helios_engine::capture::DiscoveryResult, lib_ipc::client::ClientTransportError> {
        match self.request(|command_id| EngineCommand::DiscoverDevices { command_id }, ExpectedEvent::Discovery, "discover_devices", ENGINE_RESPONSE_TIMEOUT).await? {
            EngineEvent::Discovery { discovery, .. } => Ok(discovery),
            EngineEvent::Nack { reason, .. } => Err(lib_ipc::client::ClientTransportError::Io(io::Error::other(reason))),
            other => {
                warn!(?other, "engine returned unexpected event for discover_devices after filtering");
                Err(lib_ipc::client::ClientTransportError::Io(io::Error::new(io::ErrorKind::InvalidData, "unexpected engine reply for discover_devices")))
            }
        }
    }

    pub async fn get_stream_runtime_capabilities_with_timeout(&self, timeout: Duration) -> Result<StreamRuntimeCapabilities, lib_ipc::client::ClientTransportError> {
        match self.request(|command_id| EngineCommand::GetStreamRuntimeCapabilities { command_id }, ExpectedEvent::StreamRuntimeCapabilities, "get_stream_runtime_capabilities", timeout).await? {
            EngineEvent::StreamRuntimeCapabilities { capabilities, .. } => Ok(capabilities),
            EngineEvent::Nack { reason, .. } => Err(lib_ipc::client::ClientTransportError::Io(io::Error::other(reason))),
            other => {
                warn!(?other, "engine returned unexpected event for get_stream_runtime_capabilities after filtering");
                Err(lib_ipc::client::ClientTransportError::Io(io::Error::new(io::ErrorKind::InvalidData, "unexpected engine reply for get_stream_runtime_capabilities")))
            }
        }
    }
    pub async fn validate_graph_event(&self, graph: serde_json::Value, active_features: Vec<String>, enable_lints: bool) -> Result<EngineEvent, lib_ipc::client::ClientTransportError> {
        self.request(
            |command_id| EngineCommand::ValidateGraph { command_id, graph: graph.into(), active_features, enable_lints },
            ExpectedEvent::GraphValidation,
            "validate_graph",
            ENGINE_RESPONSE_TIMEOUT,
        )
        .await
    }

    pub async fn set_graph(&self, id: uuid::Uuid, graph: serde_json::Value, pipeline_id: Option<uuid::Uuid>, output: Option<String>) -> Result<EngineEvent, lib_ipc::client::ClientTransportError> {
        self.request(|command_id| EngineCommand::SetGraph { command_id, stream_id: id, graph: graph.into(), pipeline_id, output }, ExpectedEvent::Ack, "set_graph", ENGINE_RESPONSE_TIMEOUT).await
    }

    pub async fn set_graph_patch(&self, id: uuid::Uuid, patch: serde_json::Value, pipeline_id: Option<uuid::Uuid>) -> Result<EngineEvent, lib_ipc::client::ClientTransportError> {
        self.request(|command_id| EngineCommand::SetGraphPatch { command_id, stream_id: id, patch: patch.into(), pipeline_id }, ExpectedEvent::Ack, "set_graph_patch", ENGINE_RESPONSE_TIMEOUT).await
    }

    pub async fn set_control(&self, id: uuid::Uuid, control_id: u32, value: helios_engine::capture::CaptureControlValue) -> Result<EngineEvent, lib_ipc::client::ClientTransportError> {
        self.request(|command_id| EngineCommand::SetControl { command_id, stream_id: id, control_id, value }, ExpectedEvent::Ack, "set_control", ENGINE_RESPONSE_TIMEOUT).await
    }

    pub async fn set_graph_output(&self, id: uuid::Uuid, output: Option<String>) -> Result<EngineEvent, lib_ipc::client::ClientTransportError> {
        self.request(|command_id| EngineCommand::SetGraphOutput { command_id, stream_id: id, output }, ExpectedEvent::Ack, "set_graph_output", ENGINE_RESPONSE_TIMEOUT).await
    }

    pub async fn set_pipeline_inputs(
        &self,
        id: uuid::Uuid,
        pipeline_id: Option<uuid::Uuid>,
        inputs: BTreeMap<String, Option<serde_json::Value>>,
    ) -> Result<EngineEvent, lib_ipc::client::ClientTransportError> {
        let mapped = inputs.into_iter().map(|(key, value)| (key, value.map(JsonWire::from))).collect();
        self.request(|command_id| EngineCommand::SetPipelineInputs { command_id, stream_id: id, pipeline_id, inputs: mapped }, ExpectedEvent::Ack, "set_pipeline_inputs", ENGINE_RESPONSE_TIMEOUT).await
    }

    pub async fn set_pipeline_layout(&self, id: uuid::Uuid, layout: Option<helios_engine::ipc::StreamPipelineLayout>) -> Result<EngineEvent, lib_ipc::client::ClientTransportError> {
        self.request(|command_id| EngineCommand::SetPipelineLayout { command_id, stream_id: id, layout }, ExpectedEvent::Ack, "set_pipeline_layout", ENGINE_PIPELINE_LAYOUT_TIMEOUT).await
    }

    pub async fn set_pipeline_wires(&self, id: uuid::Uuid, wires: Vec<helios_engine::ipc::StreamPipelineWire>) -> Result<EngineEvent, lib_ipc::client::ClientTransportError> {
        self.request(|command_id| EngineCommand::SetPipelineWires { command_id, stream_id: id, wires }, ExpectedEvent::Ack, "set_pipeline_wires", ENGINE_PIPELINE_LAYOUT_TIMEOUT).await
    }

    pub async fn set_graph_perf(&self, id: uuid::Uuid, pipeline_id: Option<uuid::Uuid>, enabled: bool) -> Result<EngineEvent, lib_ipc::client::ClientTransportError> {
        self.request(|command_id| EngineCommand::SetGraphPerf { command_id, stream_id: id, pipeline_id, enabled }, ExpectedEvent::Ack, "set_graph_perf", ENGINE_RESPONSE_TIMEOUT).await
    }

    pub async fn reset_graph_metrics(&self, id: uuid::Uuid, pipeline_id: Option<uuid::Uuid>) -> Result<EngineEvent, lib_ipc::client::ClientTransportError> {
        self.request(|command_id| EngineCommand::ResetGraphMetrics { command_id, stream_id: id, pipeline_id }, ExpectedEvent::Ack, "reset_graph_metrics", ENGINE_RESPONSE_TIMEOUT).await
    }

    pub async fn capture_graph_flamegraph(&self, id: uuid::Uuid, pipeline_id: Option<uuid::Uuid>, duration_ms: u64) -> Result<EngineEvent, lib_ipc::client::ClientTransportError> {
        self.request(
            |command_id| EngineCommand::CaptureGraphFlamegraph { command_id, stream_id: id, pipeline_id, duration_ms },
            ExpectedEvent::Ack,
            "capture_graph_flamegraph",
            ENGINE_RESPONSE_TIMEOUT,
        )
        .await
    }

    pub async fn list_graph_outputs_event(&self, id: uuid::Uuid) -> Result<EngineEvent, lib_ipc::client::ClientTransportError> {
        self.request(|command_id| EngineCommand::ListGraphOutputs { command_id, stream_id: id }, ExpectedEvent::GraphOutputs { stream_id: id }, "list_graph_outputs", ENGINE_RESPONSE_TIMEOUT).await
    }

    pub async fn get_graph_output_sample_event_with_mode(&self, id: uuid::Uuid, port: String, fresh: bool) -> Result<EngineEvent, lib_ipc::client::ClientTransportError> {
        self.request(
            |command_id| EngineCommand::GetGraphOutputSample { command_id, stream_id: id, port: port.clone(), fresh },
            ExpectedEvent::GraphOutputSample { stream_id: id },
            "get_graph_output_sample",
            ENGINE_RESPONSE_TIMEOUT,
        )
        .await
    }

    pub async fn get_graph_output_sample_event(&self, id: uuid::Uuid, port: String) -> Result<EngineEvent, lib_ipc::client::ClientTransportError> {
        self.get_graph_output_sample_event_with_mode(id, port, true).await
    }

    pub async fn get_cached_graph_output_sample_event(&self, id: uuid::Uuid, port: String) -> Result<EngineEvent, lib_ipc::client::ClientTransportError> {
        self.get_graph_output_sample_event_with_mode(id, port, false).await
    }

    pub async fn set_codecs(&self, id: uuid::Uuid, decoder_id: Option<String>, encoder_id: Option<String>) -> Result<EngineEvent, lib_ipc::client::ClientTransportError> {
        self.request(|command_id| EngineCommand::SetCodecs { command_id, stream_id: id, decoder_id, encoder_id }, ExpectedEvent::Ack, "set_codecs", ENGINE_RESPONSE_TIMEOUT).await
    }

    pub async fn set_calibration(&self, id: uuid::Uuid, calibration: Option<StreamCalibration>) -> Result<EngineEvent, lib_ipc::client::ClientTransportError> {
        self.request(|command_id| EngineCommand::SetCalibration { command_id, stream_id: id, calibration }, ExpectedEvent::Ack, "set_calibration", ENGINE_RESPONSE_TIMEOUT).await
    }

    pub async fn set_calibration_mode(&self, id: uuid::Uuid, enabled: bool, dictionary: Option<String>, mode: Option<String>) -> Result<EngineEvent, lib_ipc::client::ClientTransportError> {
        self.request(|command_id| EngineCommand::SetCalibrationMode { command_id, stream_id: id, enabled, dictionary, mode }, ExpectedEvent::Ack, "set_calibration_mode", ENGINE_RESPONSE_TIMEOUT).await
    }

    pub async fn solve_calibration_event(&self, request: CalibrationSolveRequest) -> Result<EngineEvent, lib_ipc::client::ClientTransportError> {
        self.request(|command_id| EngineCommand::SolveCalibration { command_id, request }, ExpectedEvent::CalibrationSolved, "solve_calibration", calibration_solve_timeout()).await
    }

    pub async fn solve_localization_event(&self, request: LocalizationSolveRequest) -> Result<EngineEvent, lib_ipc::client::ClientTransportError> {
        let request =
            serde_json::to_value(request).map(JsonWire).map_err(|err| lib_ipc::client::ClientTransportError::Io(io::Error::other(format!("failed to encode localization request: {err}"))))?;
        self.request(|command_id| EngineCommand::SolveLocalization { command_id, request }, ExpectedEvent::LocalizationSolved, "solve_localization", localization_solve_timeout()).await
    }

    pub async fn localization_pipeline_status_event(&self, request: LocalizationPipelineStatusRequest) -> Result<EngineEvent, lib_ipc::client::ClientTransportError> {
        let request = serde_json::to_value(request)
            .map(JsonWire)
            .map_err(|err| lib_ipc::client::ClientTransportError::Io(io::Error::other(format!("failed to encode localization pipeline status request: {err}"))))?;
        self.request(
            |command_id| EngineCommand::GetLocalizationPipelineStatus { command_id, request },
            ExpectedEvent::LocalizationPipelineStatus,
            "localization_pipeline_status",
            ENGINE_RESPONSE_TIMEOUT,
        )
        .await
    }

    pub async fn localization_pipeline_outputs_event(&self, request: LocalizationPipelineGraphRequest) -> Result<EngineEvent, lib_ipc::client::ClientTransportError> {
        let request = serde_json::to_value(request)
            .map(JsonWire)
            .map_err(|err| lib_ipc::client::ClientTransportError::Io(io::Error::other(format!("failed to encode localization pipeline outputs request: {err}"))))?;
        self.request(
            |command_id| EngineCommand::ListLocalizationPipelineOutputs { command_id, request },
            ExpectedEvent::LocalizationPipelineOutputs,
            "localization_pipeline_outputs",
            ENGINE_RESPONSE_TIMEOUT,
        )
        .await
    }

    pub async fn localization_pipeline_output_sample_event(&self, request: LocalizationPipelineSampleRequest) -> Result<EngineEvent, lib_ipc::client::ClientTransportError> {
        let request = serde_json::to_value(request)
            .map(JsonWire)
            .map_err(|err| lib_ipc::client::ClientTransportError::Io(io::Error::other(format!("failed to encode localization pipeline sample request: {err}"))))?;
        self.request(
            |command_id| EngineCommand::SampleLocalizationPipelineOutput { command_id, request },
            ExpectedEvent::LocalizationPipelineOutputSample,
            "localization_pipeline_output_sample",
            ENGINE_RESPONSE_TIMEOUT,
        )
        .await
    }

    pub async fn list_streams(&self) -> Result<Vec<helios_engine::ipc::StreamSummary>, lib_ipc::client::ClientTransportError> {
        self.list_streams_with_timeout(ENGINE_RESPONSE_TIMEOUT).await
    }

    pub async fn list_streams_with_timeout(&self, timeout: Duration) -> Result<Vec<helios_engine::ipc::StreamSummary>, lib_ipc::client::ClientTransportError> {
        match self.request(|command_id| EngineCommand::List { command_id }, ExpectedEvent::StreamList, "list_streams", timeout).await? {
            EngineEvent::StreamList { streams, .. } => {
                self.update_timeout_scale_from_count(streams.len());
                Ok(streams)
            }
            other => {
                warn!(?other, "engine returned unexpected event for list_streams after filtering");
                Err(lib_ipc::client::ClientTransportError::Io(io::Error::new(io::ErrorKind::InvalidData, "unexpected engine reply for list_streams")))
            }
        }
    }

    pub fn subscribe_events(&self) -> tokio::sync::broadcast::Receiver<EngineEvent> {
        self.events.subscribe()
    }

    pub fn subscribe_connect_events(&self) -> tokio::sync::broadcast::Receiver<()> {
        self.connect_events.subscribe()
    }

    pub fn is_connected(&self) -> bool {
        self.connected.load(std::sync::atomic::Ordering::Relaxed)
    }

    pub fn last_disconnect_ms(&self) -> Option<u64> {
        let ts = self.last_disconnect_ms.load(std::sync::atomic::Ordering::Relaxed);
        if ts == 0 { None } else { Some(ts) }
    }
}
