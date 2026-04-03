/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import type { LimelightControlState } from './LimelightControlState';
import type { LimelightReadSnapshot } from './LimelightReadSnapshot';
export type LimelightAdapterStatus = {
    cached_topic_count: number;
    control_state: LimelightControlState;
    publish_phase: string;
    publish_ready: boolean;
    read_snapshot: LimelightReadSnapshot;
    recording_active: boolean;
    stream_alias?: string | null;
    stream_id: string;
    stream_state: string;
    table_name: string;
    updated_at_ms: number;
};

