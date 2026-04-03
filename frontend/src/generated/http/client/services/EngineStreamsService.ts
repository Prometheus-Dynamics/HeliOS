/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import type { BenchFormatsRequest } from '../models/BenchFormatsRequest';
import type { BenchFormatsResponse } from '../models/BenchFormatsResponse';
import type { CaptureSnapshotRequest } from '../models/CaptureSnapshotRequest';
import type { CodecInfo } from '../models/CodecInfo';
import type { ControlMeta } from '../models/ControlMeta';
import type { ControlValue } from '../models/ControlValue';
import type { GraphOutputPortDescriptor } from '../models/GraphOutputPortDescriptor';
import type { MediaItem } from '../models/MediaItem';
import type { ProbedDevice } from '../models/ProbedDevice';
import type { SetPipelineGraphRequest } from '../models/SetPipelineGraphRequest';
import type { SetPipelineLayoutRequest } from '../models/SetPipelineLayoutRequest';
import type { SetPipelineOutputRequest } from '../models/SetPipelineOutputRequest';
import type { SetStreamCropRequest } from '../models/SetStreamCropRequest';
import type { SetStreamCropResponse } from '../models/SetStreamCropResponse';
import type { SmokePipelineResponse } from '../models/SmokePipelineResponse';
import type { StartRecordingRequest } from '../models/StartRecordingRequest';
import type { StartStreamResponse } from '../models/StartStreamResponse';
import type { StreamCalibrationParams } from '../models/StreamCalibrationParams';
import type { StreamCapabilitiesResponse } from '../models/StreamCapabilitiesResponse';
import type { StreamFormatInfo } from '../models/StreamFormatInfo';
import type { StreamInfo } from '../models/StreamInfo';
import type { StreamManifest } from '../models/StreamManifest';
import type { StreamMetrics } from '../models/StreamMetrics';
import type { StreamValidateResponse } from '../models/StreamValidateResponse';
import type { UpdateCameraPoseRequest } from '../models/UpdateCameraPoseRequest';
import type { UpdateStreamResponse } from '../models/UpdateStreamResponse';
import type { CancelablePromise } from '../core/CancelablePromise';
import { OpenAPI } from '../core/OpenAPI';
import { request as __request } from '../core/request';
export class EngineStreamsService {
    /**
     * @returns StreamInfo List active streams
     * @throws ApiError
     */
    public static listStreams(): CancelablePromise<Array<StreamInfo>> {
        return __request(OpenAPI, {
            method: 'GET',
            url: '/streams',
        });
    }
    /**
     * @returns StartStreamResponse Stream started
     * @throws ApiError
     */
    public static startStream({
        requestBody,
    }: {
        requestBody: StreamManifest,
    }): CancelablePromise<StartStreamResponse> {
        return __request(OpenAPI, {
            method: 'POST',
            url: '/streams',
            body: requestBody,
            mediaType: 'application/json',
            errors: {
                422: `Semantic validation failure`,
            },
        });
    }
    /**
     * @returns ProbedDevice Available capture backends/devices
     * @throws ApiError
     */
    public static listBackends(): CancelablePromise<Array<ProbedDevice>> {
        return __request(OpenAPI, {
            method: 'GET',
            url: '/streams/backends',
        });
    }
    /**
     * @returns BenchFormatsResponse Benchmark results
     * @throws ApiError
     */
    public static benchFormats({
        requestBody,
    }: {
        requestBody: BenchFormatsRequest,
    }): CancelablePromise<BenchFormatsResponse> {
        return __request(OpenAPI, {
            method: 'POST',
            url: '/streams/bench/formats',
            body: requestBody,
            mediaType: 'application/json',
        });
    }
    /**
     * @returns StreamCapabilitiesResponse Stream validation constraints and defaults
     * @throws ApiError
     */
    public static streamCapabilitiesHandler(): CancelablePromise<StreamCapabilitiesResponse> {
        return __request(OpenAPI, {
            method: 'GET',
            url: '/streams/capabilities',
            errors: {
                502: `Runtime capability inventory unavailable`,
            },
        });
    }
    /**
     * @returns CodecInfo Available codecs (encoders + decoders)
     * @throws ApiError
     */
    public static listCodecs(): CancelablePromise<Array<CodecInfo>> {
        return __request(OpenAPI, {
            method: 'GET',
            url: '/streams/codecs',
            errors: {
                502: `Runtime capability inventory unavailable`,
            },
        });
    }
    /**
     * @returns StreamValidateResponse Validated + canonicalized stream manifest
     * @throws ApiError
     */
    public static validateStream({
        requestBody,
    }: {
        requestBody: StreamManifest,
    }): CancelablePromise<StreamValidateResponse> {
        return __request(OpenAPI, {
            method: 'POST',
            url: '/streams/validate',
            body: requestBody,
            mediaType: 'application/json',
            errors: {
                422: `Semantic validation failure`,
                502: `Runtime capability inventory unavailable`,
            },
        });
    }
    /**
     * @returns StreamInfo Stream info
     * @throws ApiError
     */
    public static getStream({
        id,
    }: {
        /**
         * Stream ID
         */
        id: string,
    }): CancelablePromise<StreamInfo> {
        return __request(OpenAPI, {
            method: 'GET',
            url: '/streams/{id}',
            path: {
                'id': id,
            },
            errors: {
                404: `Stream not found`,
            },
        });
    }
    /**
     * @returns UpdateStreamResponse Stream updated
     * @throws ApiError
     */
    public static updateStream({
        id,
        requestBody,
    }: {
        /**
         * Stream ID
         */
        id: string,
        requestBody: StreamManifest,
    }): CancelablePromise<UpdateStreamResponse> {
        return __request(OpenAPI, {
            method: 'PUT',
            url: '/streams/{id}',
            path: {
                'id': id,
            },
            body: requestBody,
            mediaType: 'application/json',
            errors: {
                404: `Stream not found`,
                422: `Semantic validation failure`,
            },
        });
    }
    /**
     * @returns void
     * @throws ApiError
     */
    public static deleteStream({
        id,
    }: {
        /**
         * Stream ID
         */
        id: string,
    }): CancelablePromise<void> {
        return __request(OpenAPI, {
            method: 'DELETE',
            url: '/streams/{id}',
            path: {
                'id': id,
            },
        });
    }
    /**
     * @returns any Updated stream manifest
     * @throws ApiError
     */
    public static applyCalibration({
        id,
        requestBody,
    }: {
        id: string,
        requestBody: StreamCalibrationParams,
    }): CancelablePromise<any> {
        return __request(OpenAPI, {
            method: 'POST',
            url: '/streams/{id}/calibration/apply',
            path: {
                'id': id,
            },
            body: requestBody,
            mediaType: 'application/json',
        });
    }
    /**
     * @returns any Calibration board PNG
     * @throws ApiError
     */
    public static boardPng({
        id,
    }: {
        /**
         * Stream id
         */
        id: string,
    }): CancelablePromise<any> {
        return __request(OpenAPI, {
            method: 'GET',
            url: '/streams/{id}/calibration/board',
            path: {
                'id': id,
            },
        });
    }
    /**
     * @returns any Calibration board PDF
     * @throws ApiError
     */
    public static boardPdf({
        id,
    }: {
        /**
         * Stream id
         */
        id: string,
    }): CancelablePromise<any> {
        return __request(OpenAPI, {
            method: 'GET',
            url: '/streams/{id}/calibration/board.pdf',
            path: {
                'id': id,
            },
        });
    }
    /**
     * @returns any Updated stream manifest
     * @throws ApiError
     */
    public static saveCalibration({
        id,
        requestBody,
    }: {
        id: string,
        requestBody: StreamCalibrationParams,
    }): CancelablePromise<any> {
        return __request(OpenAPI, {
            method: 'POST',
            url: '/streams/{id}/calibration/save',
            path: {
                'id': id,
            },
            body: requestBody,
            mediaType: 'application/json',
        });
    }
    /**
     * @returns ControlMeta Current controls
     * @throws ApiError
     */
    public static getControls({
        id,
    }: {
        /**
         * Stream ID
         */
        id: string,
    }): CancelablePromise<Array<ControlMeta>> {
        return __request(OpenAPI, {
            method: 'GET',
            url: '/streams/{id}/controls',
            path: {
                'id': id,
            },
        });
    }
    /**
     * @returns void
     * @throws ApiError
     */
    public static setControl({
        id,
        controlId,
        requestBody,
    }: {
        /**
         * Stream ID
         */
        id: string,
        /**
         * Control ID
         */
        controlId: number,
        requestBody: ControlValue,
    }): CancelablePromise<void> {
        return __request(OpenAPI, {
            method: 'POST',
            url: '/streams/{id}/controls/{control_id}',
            path: {
                'id': id,
                'control_id': controlId,
            },
            body: requestBody,
            mediaType: 'application/json',
        });
    }
    /**
     * @returns SetStreamCropResponse Crop applied
     * @throws ApiError
     */
    public static setCrop({
        id,
        requestBody,
    }: {
        /**
         * Stream ID
         */
        id: string,
        requestBody: SetStreamCropRequest,
    }): CancelablePromise<SetStreamCropResponse> {
        return __request(OpenAPI, {
            method: 'POST',
            url: '/streams/{id}/crop',
            path: {
                'id': id,
            },
            body: requestBody,
            mediaType: 'application/json',
            errors: {
                400: `Invalid request`,
                404: `Stream not found`,
                502: `Engine error`,
            },
        });
    }
    /**
     * @returns StreamFormatInfo Latest encoded payload format info
     * @throws ApiError
     */
    public static streamFormat({
        id,
    }: {
        /**
         * Stream ID
         */
        id: string,
    }): CancelablePromise<StreamFormatInfo> {
        return __request(OpenAPI, {
            method: 'GET',
            url: '/streams/{id}/format',
            path: {
                'id': id,
            },
        });
    }
    /**
     * @returns any Latest JPEG frame
     * @throws ApiError
     */
    public static frameJpeg({
        id,
    }: {
        /**
         * Stream ID
         */
        id: string,
    }): CancelablePromise<any> {
        return __request(OpenAPI, {
            method: 'GET',
            url: '/streams/{id}/frame',
            path: {
                'id': id,
            },
        });
    }
    /**
     * @returns StreamMetrics Stream metrics
     * @throws ApiError
     */
    public static getMetrics({
        id,
    }: {
        /**
         * Stream ID
         */
        id: string,
    }): CancelablePromise<StreamMetrics> {
        return __request(OpenAPI, {
            method: 'GET',
            url: '/streams/{id}/metrics',
            path: {
                'id': id,
            },
        });
    }
    /**
     * @returns void
     * @throws ApiError
     */
    public static setPipelineGraph({
        id,
        requestBody,
    }: {
        /**
         * Stream ID
         */
        id: string,
        requestBody: SetPipelineGraphRequest,
    }): CancelablePromise<void> {
        return __request(OpenAPI, {
            method: 'POST',
            url: '/streams/{id}/pipeline/graph',
            path: {
                'id': id,
            },
            body: requestBody,
            mediaType: 'application/json',
        });
    }
    /**
     * @returns void
     * @throws ApiError
     */
    public static setPipelineLayout({
        id,
        requestBody,
    }: {
        /**
         * Stream ID
         */
        id: string,
        requestBody: SetPipelineLayoutRequest,
    }): CancelablePromise<void> {
        return __request(OpenAPI, {
            method: 'POST',
            url: '/streams/{id}/pipeline/layout',
            path: {
                'id': id,
            },
            body: requestBody,
            mediaType: 'application/json',
        });
    }
    /**
     * @returns void
     * @throws ApiError
     */
    public static setPipelineOutput({
        id,
        requestBody,
    }: {
        /**
         * Stream ID
         */
        id: string,
        requestBody: SetPipelineOutputRequest,
    }): CancelablePromise<void> {
        return __request(OpenAPI, {
            method: 'POST',
            url: '/streams/{id}/pipeline/output',
            path: {
                'id': id,
            },
            body: requestBody,
            mediaType: 'application/json',
        });
    }
    /**
     * @returns GraphOutputPortDescriptor Pipeline graph host outputs
     * @throws ApiError
     */
    public static listPipelineOutputs({
        id,
    }: {
        /**
         * Stream ID
         */
        id: string,
    }): CancelablePromise<Array<GraphOutputPortDescriptor>> {
        return __request(OpenAPI, {
            method: 'GET',
            url: '/streams/{id}/pipeline/outputs',
            path: {
                'id': id,
            },
        });
    }
    /**
     * @returns any Latest JSON sample captured from the port
     * @throws ApiError
     */
    public static getPipelineOutputSample({
        id,
        port,
    }: {
        /**
         * Stream ID
         */
        id: string,
        /**
         * Host output port name
         */
        port: string,
    }): CancelablePromise<any> {
        return __request(OpenAPI, {
            method: 'GET',
            url: '/streams/{id}/pipeline/outputs/{port}/sample',
            path: {
                'id': id,
                'port': port,
            },
        });
    }
    /**
     * @returns SmokePipelineResponse Runtime pipeline health snapshot
     * @throws ApiError
     */
    public static smokePipelineGraph({
        id,
        timeoutMs,
    }: {
        /**
         * Stream ID
         */
        id: string,
        timeoutMs?: number | null,
    }): CancelablePromise<SmokePipelineResponse> {
        return __request(OpenAPI, {
            method: 'GET',
            url: '/streams/{id}/pipeline/smoke',
            path: {
                'id': id,
            },
            query: {
                'timeout_ms': timeoutMs,
            },
        });
    }
    /**
     * @returns void
     * @throws ApiError
     */
    public static updateStreamPose({
        id,
        requestBody,
    }: {
        /**
         * Stream ID
         */
        id: string,
        requestBody: UpdateCameraPoseRequest,
    }): CancelablePromise<void> {
        return __request(OpenAPI, {
            method: 'PUT',
            url: '/streams/{id}/pose',
            path: {
                'id': id,
            },
            body: requestBody,
            mediaType: 'application/json',
        });
    }
    /**
     * @returns void
     * @throws ApiError
     */
    public static clearStreamPose({
        id,
    }: {
        /**
         * Stream ID
         */
        id: string,
    }): CancelablePromise<void> {
        return __request(OpenAPI, {
            method: 'DELETE',
            url: '/streams/{id}/pose',
            path: {
                'id': id,
            },
        });
    }
    /**
     * @returns any Live stream preview (MJPEG multipart or length-prefixed encoded)
     * @throws ApiError
     */
    public static previewStream({
        id,
    }: {
        /**
         * Stream ID
         */
        id: string,
    }): CancelablePromise<any> {
        return __request(OpenAPI, {
            method: 'GET',
            url: '/streams/{id}/preview',
            path: {
                'id': id,
            },
        });
    }
    /**
     * @returns MediaItem Recording started
     * @throws ApiError
     */
    public static startRecording({
        id,
        requestBody,
    }: {
        /**
         * Stream ID
         */
        id: string,
        requestBody: StartRecordingRequest,
    }): CancelablePromise<MediaItem> {
        return __request(OpenAPI, {
            method: 'POST',
            url: '/streams/{id}/recording/start',
            path: {
                'id': id,
            },
            body: requestBody,
            mediaType: 'application/json',
            errors: {
                400: `Invalid request`,
                502: `Engine error`,
            },
        });
    }
    /**
     * @returns void
     * @throws ApiError
     */
    public static stopRecording({
        id,
    }: {
        /**
         * Stream ID
         */
        id: string,
    }): CancelablePromise<void> {
        return __request(OpenAPI, {
            method: 'POST',
            url: '/streams/{id}/recording/stop',
            path: {
                'id': id,
            },
            errors: {
                502: `Engine error`,
            },
        });
    }
    /**
     * @returns MediaItem Snapshot stored
     * @throws ApiError
     */
    public static captureSnapshot({
        id,
        requestBody,
    }: {
        /**
         * Stream ID
         */
        id: string,
        requestBody: CaptureSnapshotRequest,
    }): CancelablePromise<MediaItem> {
        return __request(OpenAPI, {
            method: 'POST',
            url: '/streams/{id}/snapshot',
            path: {
                'id': id,
            },
            body: requestBody,
            mediaType: 'application/json',
            errors: {
                400: `Invalid request`,
                404: `Stream not found`,
                502: `Engine error`,
                503: `Engine unavailable`,
            },
        });
    }
}
