/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import type { StreamRecordingMode } from './StreamRecordingMode';
export type StreamValidationDefaults = {
    defaultDecoderEnabled: boolean;
    defaultDecoderIdsByCaptureFormat?: Record<string, string>;
    defaultEncoderEnabled: boolean;
    defaultEncoderId?: string | null;
    defaultHostBuffer: number;
    defaultPreviewJpegQuality: number;
    defaultPreviewJpegQualityWhenEncoderDisabled: number;
    defaultRecordingMode: StreamRecordingMode;
    defaultStartOnBoot: boolean;
    pipelineEnabledWhenBindingsPresent: boolean;
    rawOutput: string;
    undistortedOutput: string;
};

