/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import type { CaptureDescriptor } from './CaptureDescriptor';
import type { ResolvedStreamConfig } from './ResolvedStreamConfig';
import type { StreamManifest } from './StreamManifest';
import type { StreamPreviewFormat } from './StreamPreviewFormat';
import type { StreamRuntimeState } from './StreamRuntimeState';
import type { StreamStatus } from './StreamStatus';
export type StreamInfo = {
    descriptor: CaptureDescriptor;
    id: string;
    manifest: StreamManifest;
    preview_format: StreamPreviewFormat;
    resolved: ResolvedStreamConfig;
    runtime?: (null | StreamRuntimeState);
    status?: (null | StreamStatus);
};

