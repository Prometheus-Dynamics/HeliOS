/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import type { ConsoleSessionListPayload } from '../models/ConsoleSessionListPayload';
import type { ConsoleSessionSummaryPayload } from '../models/ConsoleSessionSummaryPayload';
import type { CreateConsoleSessionRequest } from '../models/CreateConsoleSessionRequest';
import type { CancelablePromise } from '../core/CancelablePromise';
import { OpenAPI } from '../core/OpenAPI';
import { request as __request } from '../core/request';
export class ConsoleService {
    /**
     * @returns ConsoleSessionListPayload List console sessions
     * @throws ApiError
     */
    public static listSessions(): CancelablePromise<ConsoleSessionListPayload> {
        return __request(OpenAPI, {
            method: 'GET',
            url: '/console/sessions',
            errors: {
                500: `Internal error`,
            },
        });
    }
    /**
     * @returns ConsoleSessionSummaryPayload Created session
     * @throws ApiError
     */
    public static createSession({
        requestBody,
    }: {
        requestBody: CreateConsoleSessionRequest,
    }): CancelablePromise<ConsoleSessionSummaryPayload> {
        return __request(OpenAPI, {
            method: 'POST',
            url: '/console/sessions',
            body: requestBody,
            mediaType: 'application/json',
            errors: {
                500: `Internal error`,
            },
        });
    }
    /**
     * @returns void
     * @throws ApiError
     */
    public static deleteSession({
        sessionId,
    }: {
        /**
         * Session UUID
         */
        sessionId: string,
    }): CancelablePromise<void> {
        return __request(OpenAPI, {
            method: 'DELETE',
            url: '/console/sessions/{session_id}',
            path: {
                'session_id': sessionId,
            },
            errors: {
                404: `Session not found`,
            },
        });
    }
}
