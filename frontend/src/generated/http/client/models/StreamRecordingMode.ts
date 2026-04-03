/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import type { RecordingCodec } from './RecordingCodec';
export type StreamRecordingMode = ({
    state: 'disabled';
} | {
    codec: RecordingCodec;
    state: 'shadow_buffer';
});

