#[allow(unused_imports)]
use tracing::error;

#[repr(u16)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EngineCommandKind {
    List = 0,
    Start = 1,
    Stop = 2,
    SetControl = 3,
    GetControls = 4,
    GetMetrics = 5,
    GetNodeRegistry = 6,
    DiscoverDevices = 7,
    ValidateGraph = 8,
    SetGraph = 9,
    SetGraphOutput = 10,
    SetCodecs = 11,
    ListGraphOutputs = 12,
    GetGraphOutputSample = 13,
    SetCalibration = 14,
    SetCalibrationMode = 15,
    SnapshotJpeg = 16,
    SetPipelineLayout = 17,
    RefreshNodeRegistry = 18,
    SetPipelineInputs = 19,
    SolveCalibration = 20,
    SetGraphPatch = 21,
    StartRecording = 22,
    StopRecording = 23,
    CaptureShadowRecording = 24,
    SetPipelineWires = 25,
    SetGraphPerf = 26,
    ResetGraphMetrics = 27,
    CaptureGraphFlamegraph = 28,
    SolveLocalization = 29,
    GetLocalizationPipelineStatus = 30,
    ListLocalizationPipelineOutputs = 31,
    SampleLocalizationPipelineOutput = 32,
}

#[repr(u16)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EngineEventKind {
    Ack = 0,
    Nack = 1,
    StreamList = 2,
    Started = 3,
    Stopped = 4,
    Controls = 5,
    Metrics = 6,
    MetricsUpdate = 7,
    NodeRegistry = 8,
    Discovery = 9,
    GraphValidation = 10,
    GraphOutputs = 11,
    GraphOutputSample = 12,
    SnapshotJpeg = 13,
    CalibrationSolved = 14,
    LocalizationSolved = 15,
    LocalizationPipelineStatus = 16,
    LocalizationPipelineOutputs = 17,
    LocalizationPipelineOutputSample = 18,
}

impl EngineCommandKind {
    const fn to_u16(self) -> u16 {
        self as u16
    }

    fn from_u16(value: u16) -> Option<Self> {
        match value {
            0 => Some(Self::List),
            1 => Some(Self::Start),
            2 => Some(Self::Stop),
            3 => Some(Self::SetControl),
            4 => Some(Self::GetControls),
            5 => Some(Self::GetMetrics),
            6 => Some(Self::GetNodeRegistry),
            7 => Some(Self::DiscoverDevices),
            8 => Some(Self::ValidateGraph),
            9 => Some(Self::SetGraph),
            10 => Some(Self::SetGraphOutput),
            11 => Some(Self::SetCodecs),
            12 => Some(Self::ListGraphOutputs),
            13 => Some(Self::GetGraphOutputSample),
            14 => Some(Self::SetCalibration),
            15 => Some(Self::SetCalibrationMode),
            16 => Some(Self::SnapshotJpeg),
            17 => Some(Self::SetPipelineLayout),
            18 => Some(Self::RefreshNodeRegistry),
            19 => Some(Self::SetPipelineInputs),
            20 => Some(Self::SolveCalibration),
            21 => Some(Self::SetGraphPatch),
            22 => Some(Self::StartRecording),
            23 => Some(Self::StopRecording),
            24 => Some(Self::CaptureShadowRecording),
            25 => Some(Self::SetPipelineWires),
            26 => Some(Self::SetGraphPerf),
            27 => Some(Self::ResetGraphMetrics),
            28 => Some(Self::CaptureGraphFlamegraph),
            29 => Some(Self::SolveLocalization),
            30 => Some(Self::GetLocalizationPipelineStatus),
            31 => Some(Self::ListLocalizationPipelineOutputs),
            32 => Some(Self::SampleLocalizationPipelineOutput),
            _ => None,
        }
    }
}

impl EngineEventKind {
    const fn to_u16(self) -> u16 {
        self as u16
    }

    fn from_u16(value: u16) -> Option<Self> {
        match value {
            0 => Some(Self::Ack),
            1 => Some(Self::Nack),
            2 => Some(Self::StreamList),
            3 => Some(Self::Started),
            4 => Some(Self::Stopped),
            5 => Some(Self::Controls),
            6 => Some(Self::Metrics),
            7 => Some(Self::MetricsUpdate),
            8 => Some(Self::NodeRegistry),
            9 => Some(Self::Discovery),
            10 => Some(Self::GraphValidation),
            11 => Some(Self::GraphOutputs),
            12 => Some(Self::GraphOutputSample),
            13 => Some(Self::SnapshotJpeg),
            14 => Some(Self::CalibrationSolved),
            15 => Some(Self::LocalizationSolved),
            16 => Some(Self::LocalizationPipelineStatus),
            17 => Some(Self::LocalizationPipelineOutputs),
            18 => Some(Self::LocalizationPipelineOutputSample),
            _ => None,
        }
    }
}

#[allow(unreachable_code)]
const _: () = {
    lib_ipc::tagged_enum! {
        impl crate::ipc::EngineCommand => crate::ipc::EngineCommandKind {
            struct List { command_id: lib_ipc::types::CommandId },
            struct Start { command_id: lib_ipc::types::CommandId => with_serde, manifest: Box<crate::ipc::StreamManifest> => with_serde },
            struct SetCodecs { command_id: lib_ipc::types::CommandId => with_serde, stream_id: uuid::Uuid => with_serde, decoder_id: Option<String> => with_serde, encoder_id: Option<String> => with_serde },
            struct SetCalibration { command_id: lib_ipc::types::CommandId => with_serde, stream_id: uuid::Uuid => with_serde, calibration: Option<crate::ipc::StreamCalibration> => with_serde },
            struct SetCalibrationMode { command_id: lib_ipc::types::CommandId => with_serde, stream_id: uuid::Uuid => with_serde, enabled: bool, dictionary: Option<String> => with_serde, mode: Option<String> => with_serde },
            struct Stop { command_id: lib_ipc::types::CommandId => with_serde, stream_id: uuid::Uuid => with_serde },
            struct SetControl { command_id: lib_ipc::types::CommandId => with_serde, stream_id: uuid::Uuid => with_serde, control_id: crate::ipc::ControlId, value: crate::capture::CaptureControlValue => with_serde },
            struct GetControls { command_id: lib_ipc::types::CommandId => with_serde, stream_id: uuid::Uuid => with_serde },
            struct GetMetrics { command_id: lib_ipc::types::CommandId => with_serde, stream_id: uuid::Uuid => with_serde },
            struct SnapshotJpeg { command_id: lib_ipc::types::CommandId => with_serde, stream_id: uuid::Uuid => with_serde, quality: u8, source: Option<crate::ipc::RecordingSource> },
            struct GetNodeRegistry { command_id: lib_ipc::types::CommandId => with_serde },
            struct DiscoverDevices { command_id: lib_ipc::types::CommandId => with_serde },
            struct RefreshNodeRegistry { command_id: lib_ipc::types::CommandId => with_serde },
            struct ValidateGraph { command_id: lib_ipc::types::CommandId => with_serde, graph: crate::ipc::JsonWire => with_serde, active_features: Vec<String> => with_serde, enable_lints: bool },
            struct SetGraph { command_id: lib_ipc::types::CommandId => with_serde, stream_id: uuid::Uuid => with_serde, graph: crate::ipc::JsonWire => with_serde, pipeline_id: Option<uuid::Uuid> => with_serde, output: Option<String> => with_serde },
            struct SetGraphPatch { command_id: lib_ipc::types::CommandId => with_serde, stream_id: uuid::Uuid => with_serde, patch: crate::ipc::JsonWire => with_serde, pipeline_id: Option<uuid::Uuid> => with_serde },
            struct SetGraphOutput { command_id: lib_ipc::types::CommandId => with_serde, stream_id: uuid::Uuid => with_serde, output: Option<String> => with_serde },
            struct SetPipelineInputs { command_id: lib_ipc::types::CommandId => with_serde, stream_id: uuid::Uuid => with_serde, pipeline_id: Option<uuid::Uuid> => with_serde, inputs: std::collections::BTreeMap<String, Option<crate::ipc::JsonWire>> => with_serde },
            struct ListGraphOutputs { command_id: lib_ipc::types::CommandId => with_serde, stream_id: uuid::Uuid => with_serde },
            struct GetGraphOutputSample { command_id: lib_ipc::types::CommandId => with_serde, stream_id: uuid::Uuid => with_serde, port: String, fresh: bool },
            struct SetPipelineLayout { command_id: lib_ipc::types::CommandId => with_serde, stream_id: uuid::Uuid => with_serde, layout: Option<crate::ipc::StreamPipelineLayout> => with_serde },
            struct SolveCalibration { command_id: lib_ipc::types::CommandId => with_serde, request: crate::ipc::CalibrationSolveRequest => with_serde },
            struct SolveLocalization { command_id: lib_ipc::types::CommandId => with_serde, request: crate::ipc::JsonWire => with_serde },
            struct GetLocalizationPipelineStatus { command_id: lib_ipc::types::CommandId => with_serde, request: crate::ipc::JsonWire => with_serde },
            struct ListLocalizationPipelineOutputs { command_id: lib_ipc::types::CommandId => with_serde, request: crate::ipc::JsonWire => with_serde },
            struct SampleLocalizationPipelineOutput { command_id: lib_ipc::types::CommandId => with_serde, request: crate::ipc::JsonWire => with_serde },
            struct StartRecording { command_id: lib_ipc::types::CommandId => with_serde, stream_id: uuid::Uuid => with_serde, source: crate::ipc::RecordingSource, output_path: String, container: crate::ipc::RecordingContainer => with_serde, codec: crate::ipc::RecordingCodec => with_serde, duration_ms: Option<u64> => with_serde, settings: Option<crate::ipc::RecordingSettings> => with_serde },
            struct StopRecording { command_id: lib_ipc::types::CommandId => with_serde, stream_id: uuid::Uuid => with_serde },
            struct CaptureShadowRecording { command_id: lib_ipc::types::CommandId => with_serde, stream_id: uuid::Uuid => with_serde, output_path: String, container: crate::ipc::RecordingContainer => with_serde, window_ms: u64 },
            struct SetPipelineWires { command_id: lib_ipc::types::CommandId => with_serde, stream_id: uuid::Uuid => with_serde, wires: Vec<crate::ipc::StreamPipelineWire> => with_serde },
            struct SetGraphPerf { command_id: lib_ipc::types::CommandId => with_serde, stream_id: uuid::Uuid => with_serde, pipeline_id: Option<uuid::Uuid> => with_serde, enabled: bool },
            struct ResetGraphMetrics { command_id: lib_ipc::types::CommandId => with_serde, stream_id: uuid::Uuid => with_serde, pipeline_id: Option<uuid::Uuid> => with_serde },
            struct CaptureGraphFlamegraph { command_id: lib_ipc::types::CommandId => with_serde, stream_id: uuid::Uuid => with_serde, pipeline_id: Option<uuid::Uuid> => with_serde, duration_ms: u64 },
        }
    }

    lib_ipc::tagged_enum! {
        impl crate::ipc::EngineEvent => crate::ipc::EngineEventKind {
            struct Ack { command_id: lib_ipc::types::CommandId => with_serde, ok: bool },
            struct Nack { command_id: lib_ipc::types::CommandId => with_serde, code: crate::ipc::EngineErrorCode, reason: String },
            struct StreamList { command_id: lib_ipc::types::CommandId => with_serde, streams: Vec<crate::ipc::StreamSummary> => with_serde },
            struct Started { command_id: lib_ipc::types::CommandId => with_serde, stream_id: uuid::Uuid => with_serde, descriptor: crate::capture::CaptureDescriptor => with_serde },
            struct Stopped { command_id: lib_ipc::types::CommandId => with_serde, stream_id: uuid::Uuid => with_serde },
            struct Controls { command_id: lib_ipc::types::CommandId => with_serde, stream_id: uuid::Uuid => with_serde, controls: Vec<crate::capture::CaptureControlInfo> => with_serde },
            struct Metrics { command_id: lib_ipc::types::CommandId => with_serde, stream_id: uuid::Uuid => with_serde, metrics: crate::stream::StreamMetrics => with_serde },
            struct SnapshotJpeg { command_id: lib_ipc::types::CommandId => with_serde, stream_id: uuid::Uuid => with_serde, quality: u8, bytes: Vec<u8> => with_serde },
            struct MetricsUpdate { stream_id: uuid::Uuid => with_serde, metrics: crate::stream::StreamMetrics => with_serde },
            struct GraphOutputs { command_id: lib_ipc::types::CommandId => with_serde, stream_id: uuid::Uuid => with_serde, outputs: Vec<crate::ipc::GraphOutputPortDescriptor> => with_serde },
            struct GraphOutputSample { command_id: lib_ipc::types::CommandId => with_serde, stream_id: uuid::Uuid => with_serde, port: String, value: crate::ipc::JsonWire => with_serde },
            struct NodeRegistry { command_id: lib_ipc::types::CommandId => with_serde, snapshot: crate::ipc::NodeRegistrySnapshot => with_serde },
            struct Discovery { command_id: lib_ipc::types::CommandId => with_serde, discovery: crate::capture::DiscoveryResult => with_serde },
            struct GraphValidation { command_id: lib_ipc::types::CommandId => with_serde, report: crate::ipc::GraphValidationReport => with_serde },
            struct CalibrationSolved { command_id: lib_ipc::types::CommandId => with_serde, response: crate::ipc::CalibrationSolveResponse => with_serde },
            struct LocalizationSolved { command_id: lib_ipc::types::CommandId => with_serde, response: crate::ipc::JsonWire => with_serde },
            struct LocalizationPipelineStatus { command_id: lib_ipc::types::CommandId => with_serde, response: crate::ipc::JsonWire => with_serde },
            struct LocalizationPipelineOutputs { command_id: lib_ipc::types::CommandId => with_serde, outputs: Vec<String> => with_serde },
            struct LocalizationPipelineOutputSample { command_id: lib_ipc::types::CommandId => with_serde, response: crate::ipc::JsonWire => with_serde },
        }
    }
};

#[cfg(test)]
mod tests {
    use crate::ipc::{EngineCommand, RecordingSource};
    use lib_ipc::types::CommandId;
    use uuid::Uuid;

    fn assert_snapshot_roundtrip(source: Option<RecordingSource>) {
        let command = EngineCommand::SnapshotJpeg { command_id: CommandId::from_uuid(Uuid::from_u128(0x100)), stream_id: Uuid::from_u128(0x200), quality: 87, source: source.clone() };
        let envelope: lib_ipc::envelope::TaggedEnvelope = (&command).into();
        let decoded = EngineCommand::try_from(envelope).expect("snapshot command must decode");
        match decoded {
            EngineCommand::SnapshotJpeg { command_id, stream_id, quality, source: decoded_source } => {
                assert_eq!(command_id, CommandId::from_uuid(Uuid::from_u128(0x100)));
                assert_eq!(stream_id, Uuid::from_u128(0x200));
                assert_eq!(quality, 87);
                match (decoded_source, source) {
                    (None, None) => {}
                    (Some(RecordingSource::Multiplex), Some(RecordingSource::Multiplex)) => {}
                    (Some(RecordingSource::Raw), Some(RecordingSource::Raw)) => {}
                    (
                        Some(RecordingSource::Pipeline { pipeline_id: decoded_pipeline_id, output_key: decoded_output_key }),
                        Some(RecordingSource::Pipeline { pipeline_id: expected_pipeline_id, output_key: expected_output_key }),
                    ) => {
                        assert_eq!(decoded_pipeline_id, expected_pipeline_id);
                        assert_eq!(decoded_output_key, expected_output_key);
                    }
                    (decoded, expected) => panic!("snapshot source changed in roundtrip: decoded={decoded:?}, expected={expected:?}"),
                }
            }
            other => panic!("unexpected decoded command: {other:?}"),
        }
    }

    #[test]
    fn snapshot_command_roundtrips_all_sources() {
        assert_snapshot_roundtrip(None);
        assert_snapshot_roundtrip(Some(RecordingSource::Multiplex));
        assert_snapshot_roundtrip(Some(RecordingSource::Raw));
        assert_snapshot_roundtrip(Some(RecordingSource::Pipeline { pipeline_id: Some(Uuid::from_u128(0x300)), output_key: Some("overlay".to_string()) }));
    }
}
