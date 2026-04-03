/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import type { DaedalusRegistryResponse } from '../models/DaedalusRegistryResponse';
import type { PipelineDocument } from '../models/PipelineDocument';
import type { PipelineSummary } from '../models/PipelineSummary';
import type { PipelineTemplateDocument } from '../models/PipelineTemplateDocument';
import type { PipelineTemplateSummary } from '../models/PipelineTemplateSummary';
import type { UploadGraphRequest } from '../models/UploadGraphRequest';
import type { ValidateGraphRequest } from '../models/ValidateGraphRequest';
import type { ValidateGraphResponse } from '../models/ValidateGraphResponse';
import type { CancelablePromise } from '../core/CancelablePromise';
import { OpenAPI } from '../core/OpenAPI';
import { request as __request } from '../core/request';
export class PipelinesService {
    /**
     * @returns PipelineSummary List stored graphs
     * @throws ApiError
     */
    public static listGraphs(): CancelablePromise<Array<PipelineSummary>> {
        return __request(OpenAPI, {
            method: 'GET',
            url: '/pipelines/graphs',
            errors: {
                500: `Storage error`,
            },
        });
    }
    /**
     * @returns PipelineDocument Graph stored
     * @throws ApiError
     */
    public static uploadGraph({
        requestBody,
    }: {
        requestBody: UploadGraphRequest,
    }): CancelablePromise<PipelineDocument> {
        return __request(OpenAPI, {
            method: 'POST',
            url: '/pipelines/graphs',
            body: requestBody,
            mediaType: 'application/json',
            errors: {
                400: `Invalid payload`,
                500: `Storage error`,
            },
        });
    }
    /**
     * @returns PipelineDocument Graph document
     * @throws ApiError
     */
    public static fetchGraph({
        id,
    }: {
        /**
         * Graph identifier
         */
        id: string,
    }): CancelablePromise<PipelineDocument> {
        return __request(OpenAPI, {
            method: 'GET',
            url: '/pipelines/graphs/{id}',
            path: {
                'id': id,
            },
            errors: {
                404: `Graph not found`,
            },
        });
    }
    /**
     * @returns PipelineDocument Graph updated
     * @throws ApiError
     */
    public static updateGraph({
        id,
        requestBody,
    }: {
        /**
         * Graph identifier
         */
        id: string,
        requestBody: UploadGraphRequest,
    }): CancelablePromise<PipelineDocument> {
        return __request(OpenAPI, {
            method: 'PUT',
            url: '/pipelines/graphs/{id}',
            path: {
                'id': id,
            },
            body: requestBody,
            mediaType: 'application/json',
            errors: {
                400: `Invalid payload`,
                404: `Graph not found`,
                500: `Storage error`,
            },
        });
    }
    /**
     * @returns void
     * @throws ApiError
     */
    public static deleteGraph({
        id,
    }: {
        /**
         * Graph identifier
         */
        id: string,
    }): CancelablePromise<void> {
        return __request(OpenAPI, {
            method: 'DELETE',
            url: '/pipelines/graphs/{id}',
            path: {
                'id': id,
            },
            errors: {
                404: `Graph not found`,
                500: `Storage error`,
            },
        });
    }
    /**
     * @returns DaedalusRegistryResponse Daedalus node registry
     * @throws ApiError
     */
    public static listRegistry(): CancelablePromise<DaedalusRegistryResponse> {
        return __request(OpenAPI, {
            method: 'GET',
            url: '/pipelines/registry',
        });
    }
    /**
     * @returns PipelineTemplateSummary List template graphs
     * @throws ApiError
     */
    public static listTemplates(): CancelablePromise<Array<PipelineTemplateSummary>> {
        return __request(OpenAPI, {
            method: 'GET',
            url: '/pipelines/templates',
        });
    }
    /**
     * @returns PipelineTemplateDocument Template graph document
     * @throws ApiError
     */
    public static fetchTemplate({
        id,
    }: {
        /**
         * Template identifier
         */
        id: string,
    }): CancelablePromise<PipelineTemplateDocument> {
        return __request(OpenAPI, {
            method: 'GET',
            url: '/pipelines/templates/{id}',
            path: {
                'id': id,
            },
            errors: {
                404: `Template not found`,
            },
        });
    }
    /**
     * @returns ValidateGraphResponse Planner diagnostics
     * @throws ApiError
     */
    public static validateGraph({
        requestBody,
    }: {
        requestBody: ValidateGraphRequest,
    }): CancelablePromise<ValidateGraphResponse> {
        return __request(OpenAPI, {
            method: 'POST',
            url: '/pipelines/validate',
            body: requestBody,
            mediaType: 'application/json',
            errors: {
                400: `Engine validation rejected the graph`,
                502: `Planner error`,
            },
        });
    }
}
