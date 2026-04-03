/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import type { PluginInstallRequest } from '../models/PluginInstallRequest';
import type { PluginInstallResponse } from '../models/PluginInstallResponse';
import type { PluginListResponse } from '../models/PluginListResponse';
import type { PluginToggleResponse } from '../models/PluginToggleResponse';
import type { PluginUploadResponse } from '../models/PluginUploadResponse';
import type { CancelablePromise } from '../core/CancelablePromise';
import { OpenAPI } from '../core/OpenAPI';
import { request as __request } from '../core/request';
export class PluginsService {
    /**
     * @returns PluginListResponse List plugins
     * @throws ApiError
     */
    public static listPlugins(): CancelablePromise<PluginListResponse> {
        return __request(OpenAPI, {
            method: 'GET',
            url: '/plugins',
        });
    }
    /**
     * @returns any Download disabled plugin
     * @throws ApiError
     */
    public static downloadDisabled({
        name,
    }: {
        /**
         * Disabled plugin filename
         */
        name: string,
    }): CancelablePromise<any> {
        return __request(OpenAPI, {
            method: 'GET',
            url: '/plugins/disabled/{name}',
            path: {
                'name': name,
            },
            errors: {
                404: `Not found`,
            },
        });
    }
    /**
     * @returns PluginInstallResponse Plugin installed
     * @throws ApiError
     */
    public static installPlugin({
        requestBody,
    }: {
        requestBody: PluginInstallRequest,
    }): CancelablePromise<PluginInstallResponse> {
        return __request(OpenAPI, {
            method: 'POST',
            url: '/plugins/install',
            body: requestBody,
            mediaType: 'application/json',
            errors: {
                400: `Invalid request`,
                500: `Storage error`,
            },
        });
    }
    /**
     * @returns PluginUploadResponse Plugin uploaded
     * @throws ApiError
     */
    public static uploadPlugin({
        requestBody,
    }: {
        /**
         * Multipart form-data with exactly one .so file part
         */
        requestBody: string,
    }): CancelablePromise<PluginUploadResponse> {
        return __request(OpenAPI, {
            method: 'POST',
            url: '/plugins/upload',
            body: requestBody,
            mediaType: 'text/plain',
            errors: {
                400: `Invalid upload`,
                500: `Storage error`,
            },
        });
    }
    /**
     * @returns any Download uploaded plugin
     * @throws ApiError
     */
    public static downloadUpload({
        name,
    }: {
        /**
         * Uploaded plugin filename
         */
        name: string,
    }): CancelablePromise<any> {
        return __request(OpenAPI, {
            method: 'GET',
            url: '/plugins/uploads/{name}',
            path: {
                'name': name,
            },
            errors: {
                404: `Not found`,
            },
        });
    }
    /**
     * @returns any Download plugin
     * @throws ApiError
     */
    public static downloadPlugin({
        name,
    }: {
        /**
         * Plugin filename
         */
        name: string,
    }): CancelablePromise<any> {
        return __request(OpenAPI, {
            method: 'GET',
            url: '/plugins/{name}',
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
    public static deletePlugin({
        name,
    }: {
        /**
         * Plugin filename
         */
        name: string,
    }): CancelablePromise<void> {
        return __request(OpenAPI, {
            method: 'DELETE',
            url: '/plugins/{name}',
            path: {
                'name': name,
            },
            errors: {
                404: `Not found`,
            },
        });
    }
    /**
     * @returns PluginToggleResponse Plugin disabled
     * @throws ApiError
     */
    public static disablePlugin({
        name,
    }: {
        /**
         * Plugin filename
         */
        name: string,
    }): CancelablePromise<PluginToggleResponse> {
        return __request(OpenAPI, {
            method: 'POST',
            url: '/plugins/{name}/disable',
            path: {
                'name': name,
            },
            errors: {
                404: `Not found`,
            },
        });
    }
    /**
     * @returns PluginToggleResponse Plugin enabled
     * @throws ApiError
     */
    public static enablePlugin({
        name,
    }: {
        /**
         * Plugin filename
         */
        name: string,
    }): CancelablePromise<PluginToggleResponse> {
        return __request(OpenAPI, {
            method: 'POST',
            url: '/plugins/{name}/enable',
            path: {
                'name': name,
            },
            errors: {
                404: `Not found`,
            },
        });
    }
}
