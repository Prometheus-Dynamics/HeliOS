/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import type { Nt4TopicsRequest } from '../models/Nt4TopicsRequest';
import type { Nt4TopicsResponse } from '../models/Nt4TopicsResponse';
import type { Nt4ValueRequest } from '../models/Nt4ValueRequest';
import type { Nt4ValueResponse } from '../models/Nt4ValueResponse';
import type { CancelablePromise } from '../core/CancelablePromise';
import { OpenAPI } from '../core/OpenAPI';
import { request as __request } from '../core/request';
export class Nt4Service {
    /**
     * @returns Nt4TopicsResponse Discovered topics
     * @throws ApiError
     */
    public static listTopics({
        requestBody,
    }: {
        requestBody: Nt4TopicsRequest,
    }): CancelablePromise<Nt4TopicsResponse> {
        return __request(OpenAPI, {
            method: 'POST',
            url: '/nt4/topics',
            body: requestBody,
            mediaType: 'application/json',
            errors: {
                400: `Invalid payload`,
                502: `NT4 error`,
            },
        });
    }
    /**
     * @returns Nt4ValueResponse Topic value
     * @throws ApiError
     */
    public static readValue({
        requestBody,
    }: {
        requestBody: Nt4ValueRequest,
    }): CancelablePromise<Nt4ValueResponse> {
        return __request(OpenAPI, {
            method: 'POST',
            url: '/nt4/value',
            body: requestBody,
            mediaType: 'application/json',
            errors: {
                400: `Invalid payload`,
                404: `No value available`,
                502: `NT4 error`,
            },
        });
    }
}
