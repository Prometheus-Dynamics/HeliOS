/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import type { ApiRealtimeDiagnostics } from './ApiRealtimeDiagnostics';
import type { CvRuntimeScratchMetricSnapshot } from './CvRuntimeScratchMetricSnapshot';
import type { EngineIpcObservabilitySnapshot } from './EngineIpcObservabilitySnapshot';
import type { HealthPayload } from './HealthPayload';
import type { OsReleaseInfo } from './OsReleaseInfo';
import type { ReadModelFreshness } from './ReadModelFreshness';
import type { ResourceGuardStatus } from './ResourceGuardStatus';
import type { RuntimeBroadcastSnapshot } from './RuntimeBroadcastSnapshot';
import type { RuntimeStreamsPayload } from './RuntimeStreamsPayload';
import type { RuntimeTopicBroadcastSnapshot } from './RuntimeTopicBroadcastSnapshot';
export type DeviceRuntimeObservabilitySnapshot = {
    api_realtime: ApiRealtimeDiagnostics;
    cv_runtime_scratch_high_water: Array<CvRuntimeScratchMetricSnapshot>;
    engine_ipc: EngineIpcObservabilitySnapshot;
    health: HealthPayload;
    log_source_count: number;
    log_sources_freshness: ReadModelFreshness;
    log_sources_revision: number;
    mjpeg: RuntimeTopicBroadcastSnapshot;
    os: OsReleaseInfo;
    realtime_updates: RuntimeBroadcastSnapshot;
    resource_guard: ResourceGuardStatus;
    streams: RuntimeStreamsPayload;
};

