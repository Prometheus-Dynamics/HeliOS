/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import type { ApplyUpdateRequest } from '../models/ApplyUpdateRequest';
import type { CancelUpdateRequest } from '../models/CancelUpdateRequest';
import type { StageUpdateRequest } from '../models/StageUpdateRequest';
import type { UpdateAckResponse } from '../models/UpdateAckResponse';
import type { UpdateStateResponse } from '../models/UpdateStateResponse';
import type { UploadUpdateResponse } from '../models/UploadUpdateResponse';
import type { CancelablePromise } from '../core/CancelablePromise';
import { OpenAPI } from '../core/OpenAPI';
import { request as __request } from '../core/request';
export class OtaService {
    /**
     * @returns UpdateAckResponse Apply scheduled
     * @throws ApiError
     */
    public static applyUpdate({
        requestBody,
    }: {
        requestBody: ApplyUpdateRequest,
    }): CancelablePromise<UpdateAckResponse> {
        return __request(OpenAPI, {
            method: 'POST',
            url: '/ota/apply',
            body: requestBody,
            mediaType: 'application/json',
            errors: {
                400: `Invalid request`,
                503: `Updater unavailable`,
            },
        });
    }
    /**
     * @returns UpdateAckResponse Update canceled
     * @throws ApiError
     */
    public static cancelUpdate({
        requestBody,
    }: {
        requestBody: CancelUpdateRequest,
    }): CancelablePromise<UpdateAckResponse> {
        return __request(OpenAPI, {
            method: 'POST',
            url: '/ota/cancel',
            body: requestBody,
            mediaType: 'application/json',
            errors: {
                400: `Invalid request`,
                503: `Updater unavailable`,
            },
        });
    }
    /**
     * @returns UpdateAckResponse Update staged
     * @throws ApiError
     */
    public static stageUpdate({
        requestBody,
    }: {
        requestBody: StageUpdateRequest,
    }): CancelablePromise<UpdateAckResponse> {
        return __request(OpenAPI, {
            method: 'POST',
            url: '/ota/stage',
            body: requestBody,
            mediaType: 'application/json',
            errors: {
                400: `Invalid request`,
                503: `Updater unavailable`,
            },
        });
    }
    /**
     * @returns UpdateStateResponse Current updater state
     * @throws ApiError
     */
    public static updaterState(): CancelablePromise<UpdateStateResponse> {
        return __request(OpenAPI, {
            method: 'GET',
            url: '/ota/state',
            errors: {
                503: `Updater unavailable`,
            },
        });
    }
    /**
     * @returns UploadUpdateResponse Image uploaded
     * @throws ApiError
     */
    public static uploadUpdate({
        requestBody,
    }: {
        requestBody: string,
    }): CancelablePromise<UploadUpdateResponse> {
        return __request(OpenAPI, {
            method: 'POST',
            url: '/ota/upload',
            body: requestBody,
            mediaType: 'text/plain',
            errors: {
                400: `Invalid upload`,
                500: `Storage error`,
            },
        });
    }
}
