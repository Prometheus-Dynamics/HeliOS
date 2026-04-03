/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import type { RuntimeBroadcastSnapshot } from './RuntimeBroadcastSnapshot';
import type { RuntimeTopicBroadcastSnapshot } from './RuntimeTopicBroadcastSnapshot';
export type ApiRealtimeDiagnostics = {
    device_updates: RuntimeBroadcastSnapshot;
    processes: RuntimeBroadcastSnapshot;
    stream_metrics: RuntimeTopicBroadcastSnapshot;
    stream_outputs: RuntimeTopicBroadcastSnapshot;
    telemetry: RuntimeBroadcastSnapshot;
};

