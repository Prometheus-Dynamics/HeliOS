/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
export type EngineIpcObservabilitySnapshot = {
    active_streams: number;
    completed_roundtrips: number;
    connect_count: number;
    connect_event_subscribers: number;
    connected: boolean;
    disconnect_count: number;
    disconnected_pending_requests: number;
    event_subscribers: number;
    last_disconnect_ms?: number | null;
    no_subscriber_event_drops: number;
    pending_requests: number;
    pending_requests_high_water: number;
    request_queue_capacity: number;
    request_queue_depth: number;
    request_queue_high_water: number;
    request_send_failures: number;
    request_send_timeouts: number;
    request_timeouts: number;
    roundtrip_max_ms: number;
    roundtrip_total_ms: number;
    stale_response_drops: number;
    timeout_scale_ppm: number;
    unsolicited_events: number;
};

