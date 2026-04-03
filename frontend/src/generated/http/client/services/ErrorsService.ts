/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import type { ErrorHistoryResponse } from '../models/ErrorHistoryResponse';
import type { CancelablePromise } from '../core/CancelablePromise';
import { OpenAPI } from '../core/OpenAPI';
import { request as __request } from '../core/request';
export class ErrorsService {
    /**
     * @returns ErrorHistoryResponse Error history
     * @throws ApiError
     */
    public static listHistory({
        limit,
        sinceMs,
    }: {
        /**
         * Maximum number of entries
         */
        limit?: number,
        /**
         * Only include entries since this timestamp (ms)
         */
        sinceMs?: number,
    }): CancelablePromise<ErrorHistoryResponse> {
        return __request(OpenAPI, {
            method: 'GET',
            url: '/errors',
            query: {
                'limit': limit,
                'since_ms': sinceMs,
            },
        });
    }
    /**
     * @returns void
     * @throws ApiError
     */
    public static clearHistory(): CancelablePromise<void> {
        return __request(OpenAPI, {
            method: 'DELETE',
            url: '/errors',
        });
    }
    /**
     * @returns ErrorHistoryResponse Error history export
     * @throws ApiError
     */
    public static exportHistory({
        limit,
        sinceMs,
    }: {
        /**
         * Maximum number of entries
         */
        limit?: number,
        /**
         * Only include entries since this timestamp (ms)
         */
        sinceMs?: number,
    }): CancelablePromise<ErrorHistoryResponse> {
        return __request(OpenAPI, {
            method: 'GET',
            url: '/errors/export',
            query: {
                'limit': limit,
                'since_ms': sinceMs,
            },
        });
    }
}
