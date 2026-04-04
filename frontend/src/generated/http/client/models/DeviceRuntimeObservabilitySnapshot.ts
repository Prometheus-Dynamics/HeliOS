/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import type { ApiRealtimeDiagnostics } from './ApiRealtimeDiagnostics';
import type { CvRuntimeScratchMetricSnapshot } from './CvRuntimeScratchMetricSnapshot';
import type { EngineIpcObservabilitySnapshot } from './EngineIpcObservabilitySnapshot';
import type { HealthPayload } from './HealthPayload';
import type { ImuRuntimeObservabilitySnapshot } from './ImuRuntimeObservabilitySnapshot';
import type { LocalizationObservabilitySnapshot } from './LocalizationObservabilitySnapshot';
import type { Nt4ObservabilitySnapshot } from './Nt4ObservabilitySnapshot';
import type { OsReleaseInfo } from './OsReleaseInfo';
import type { ReadModelFreshness } from './ReadModelFreshness';
import type { ResourceGuardStatus } from './ResourceGuardStatus';
import type { RuntimeBroadcastSnapshot } from './RuntimeBroadcastSnapshot';
import type { RuntimeLockRegistrySnapshot } from './RuntimeLockRegistrySnapshot';
import type { RuntimeStreamsPayload } from './RuntimeStreamsPayload';
import type { RuntimeTopicBroadcastSnapshot } from './RuntimeTopicBroadcastSnapshot';
import type { StreamRuntimeCapabilitiesCacheSnapshot } from './StreamRuntimeCapabilitiesCacheSnapshot';
export type DeviceRuntimeObservabilitySnapshot = {
    api_realtime: ApiRealtimeDiagnostics;
    cv_runtime_scratch_high_water: Array<CvRuntimeScratchMetricSnapshot>;
    engine_ipc: EngineIpcObservabilitySnapshot;
    health: HealthPayload;
    imu: ImuRuntimeObservabilitySnapshot;
    json_store_locks: RuntimeLockRegistrySnapshot;
    localization: LocalizationObservabilitySnapshot;
    log_source_count: number;
    log_sources_freshness: ReadModelFreshness;
    log_sources_revision: number;
    mjpeg: RuntimeTopicBroadcastSnapshot;
    nt4: Nt4ObservabilitySnapshot;
    os: OsReleaseInfo;
    realtime_updates: RuntimeBroadcastSnapshot;
    resource_guard: ResourceGuardStatus;
    snapshot_locks: RuntimeLockRegistrySnapshot;
    stream_runtime_capabilities_cache: StreamRuntimeCapabilitiesCacheSnapshot;
    streams: RuntimeStreamsPayload;
};

