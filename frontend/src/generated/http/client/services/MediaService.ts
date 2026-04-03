/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import type { ImageEditsRequest } from '../models/ImageEditsRequest';
import type { MediaItem } from '../models/MediaItem';
import type { UpdateMetadataRequest } from '../models/UpdateMetadataRequest';
import type { CancelablePromise } from '../core/CancelablePromise';
import { OpenAPI } from '../core/OpenAPI';
import { request as __request } from '../core/request';
export class MediaService {
    /**
     * @returns MediaItem List stored media assets
     * @throws ApiError
     */
    public static listMedia(): CancelablePromise<Array<MediaItem>> {
        return __request(OpenAPI, {
            method: 'GET',
            url: '/media',
            errors: {
                500: `Storage error`,
            },
        });
    }
    /**
     * @returns MediaItem Media uploaded
     * @throws ApiError
     */
    public static uploadMedia({
        requestBody,
    }: {
        /**
         * Multipart form-data with exactly one file part (other fields are ignored)
         */
        requestBody: string,
    }): CancelablePromise<MediaItem> {
        return __request(OpenAPI, {
            method: 'POST',
            url: '/media',
            body: requestBody,
            mediaType: 'text/plain',
            errors: {
                400: `Invalid upload`,
                500: `Storage error`,
            },
        });
    }
    /**
     * @returns any Media content
     * @throws ApiError
     */
    public static fetchMedia({
        name,
    }: {
        /**
         * Media file name
         */
        name: string,
    }): CancelablePromise<any> {
        return __request(OpenAPI, {
            method: 'GET',
            url: '/media/{name}',
            path: {
                'name': name,
            },
            errors: {
                404: `Not found`,
            },
        });
    }
    /**
     * @returns void
     * @throws ApiError
     */
    public static deleteMedia({
        name,
    }: {
        /**
         * Media file name
         */
        name: string,
    }): CancelablePromise<void> {
        return __request(OpenAPI, {
            method: 'DELETE',
            url: '/media/{name}',
            path: {
                'name': name,
            },
            errors: {
                404: `Not found`,
            },
        });
    }
    /**
     * @returns MediaItem Image updated
     * @throws ApiError
     */
    public static applyImageEdits({
        name,
        requestBody,
    }: {
        /**
         * Media file name
         */
        name: string,
        requestBody: ImageEditsRequest,
    }): CancelablePromise<MediaItem> {
        return __request(OpenAPI, {
            method: 'POST',
            url: '/media/{name}/image/edits',
            path: {
                'name': name,
            },
            body: requestBody,
            mediaType: 'application/json',
            errors: {
                400: `Invalid request`,
            },
        });
    }
    /**
     * @returns any IMU sidecar stream
     * @throws ApiError
     */
    public static fetchMediaImu({
        name,
    }: {
        /**
         * Media file name
         */
        name: string,
    }): CancelablePromise<any> {
        return __request(OpenAPI, {
            method: 'GET',
            url: '/media/{name}/imu',
            path: {
                'name': name,
            },
            errors: {
                404: `Not found`,
            },
        });
    }
    /**
     * @returns MediaItem IMU sidecar attached
     * @throws ApiError
     */
    public static attachMediaImu({
        name,
        requestBody,
    }: {
        /**
         * Media file name
         */
        name: string,
        /**
         * Multipart form-data with an IMU sidecar file part named 'imu'
         */
        requestBody: string,
    }): CancelablePromise<MediaItem> {
        return __request(OpenAPI, {
            method: 'POST',
            url: '/media/{name}/imu',
            path: {
                'name': name,
            },
            body: requestBody,
            mediaType: 'text/plain',
            errors: {
                400: `Invalid upload`,
            },
        });
    }
    /**
     * @returns MediaItem IMU sidecar detached
     * @throws ApiError
     */
    public static deleteMediaImu({
        name,
    }: {
        /**
         * Media file name
         */
        name: string,
    }): CancelablePromise<MediaItem> {
        return __request(OpenAPI, {
            method: 'DELETE',
            url: '/media/{name}/imu',
            path: {
                'name': name,
            },
            errors: {
                404: `Not found`,
            },
        });
    }
    /**
     * @returns any Label file contents
     * @throws ApiError
     */
    public static fetchLabel({
        name,
    }: {
        /**
         * Media file name
         */
        name: string,
    }): CancelablePromise<any> {
        return __request(OpenAPI, {
            method: 'GET',
            url: '/media/{name}/label',
            path: {
                'name': name,
            },
            errors: {
                404: `Not found`,
            },
        });
    }
    /**
     * @returns MediaItem Label attached
     * @throws ApiError
     */
    public static attachLabel({
        name,
        requestBody,
    }: {
        /**
         * Media file name
         */
        name: string,
        /**
         * Multipart form-data with a label file part named 'label'
         */
        requestBody: string,
    }): CancelablePromise<MediaItem> {
        return __request(OpenAPI, {
            method: 'POST',
            url: '/media/{name}/label',
            path: {
                'name': name,
            },
            body: requestBody,
            mediaType: 'text/plain',
            errors: {
                400: `Invalid upload`,
            },
        });
    }
    /**
     * @returns MediaItem Metadata updated
     * @throws ApiError
     */
    public static updateMetadata({
        name,
        requestBody,
    }: {
        /**
         * Media file name
         */
        name: string,
        requestBody: UpdateMetadataRequest,
    }): CancelablePromise<MediaItem> {
        return __request(OpenAPI, {
            method: 'PATCH',
            url: '/media/{name}/metadata',
            path: {
                'name': name,
            },
            body: requestBody,
            mediaType: 'application/json',
            errors: {
                400: `Invalid request`,
            },
        });
    }
}
