/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import type { PeerIntegrationKind } from './PeerIntegrationKind';
import type { PeerRemoteRigPose } from './PeerRemoteRigPose';
import type { PeerStreamOutputSummary } from './PeerStreamOutputSummary';
import type { StreamPreviewFormat } from './StreamPreviewFormat';
export type PeerRemoteStreamSummary = {
    active_pipeline_id?: string | null;
    active_pipeline_output?: string | null;
    backend?: string | null;
    camera_uid?: string | null;
    display_name?: string | null;
    imu_output_keys?: Array<string>;
    outputs?: Array<PeerStreamOutputSummary>;
    peer_alias?: string | null;
    peer_id: string;
    peer_kind: PeerIntegrationKind;
    pose?: (null | PeerRemoteRigPose);
    preview_format: StreamPreviewFormat;
    proxy_format_url: string;
    proxy_frame_url: string;
    proxy_preview_url: string;
    remote_stream_id: string;
    state?: string | null;
    stream_alias?: string | null;
    stream_ref: string;
};

