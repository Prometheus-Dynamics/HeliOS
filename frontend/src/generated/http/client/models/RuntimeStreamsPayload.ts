/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import type { CodecInfo } from './CodecInfo';
import type { StreamCapabilitiesResponse } from './StreamCapabilitiesResponse';
import type { StreamInfo } from './StreamInfo';
export type RuntimeStreamsPayload = {
    capabilities: StreamCapabilitiesResponse;
    codecs: Array<CodecInfo>;
    resolvedStreams: Array<StreamInfo>;
    revision: number;
    stale: boolean;
};

