/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import type { ExternalLocalizationSample } from '../models/ExternalLocalizationSample';
import type { ExternalLocalizationSampleRequest } from '../models/ExternalLocalizationSampleRequest';
import type { ExternalLocalizationSource } from '../models/ExternalLocalizationSource';
import type { ExternalLocalizationSourceUpsert } from '../models/ExternalLocalizationSourceUpsert';
import type { FieldMapDocument } from '../models/FieldMapDocument';
import type { FieldMapSummary } from '../models/FieldMapSummary';
import type { LocalizationCapabilitiesResponse } from '../models/LocalizationCapabilitiesResponse';
import type { LocalizationConfig } from '../models/LocalizationConfig';
import type { LocalizationPipelineSource } from '../models/LocalizationPipelineSource';
import type { LocalizationProfilesExportEnvelope } from '../models/LocalizationProfilesExportEnvelope';
import type { LocalizationProfilesImportRequest } from '../models/LocalizationProfilesImportRequest';
import type { LocalizationSolveResponse } from '../models/LocalizationSolveResponse';
import type { LocalizationValidateResponse } from '../models/LocalizationValidateResponse';
import type { PipelineOutputSample } from '../models/PipelineOutputSample';
import type { CancelablePromise } from '../core/CancelablePromise';
import { OpenAPI } from '../core/OpenAPI';
import { request as __request } from '../core/request';
export class LocalizationService {
    /**
     * @returns LocalizationCapabilitiesResponse Localization validation constraints and defaults
     * @throws ApiError
     */
    public static localizationCapabilitiesHandler(): CancelablePromise<LocalizationCapabilitiesResponse> {
        return __request(OpenAPI, {
            method: 'GET',
            url: '/localization/capabilities',
        });
    }
    /**
     * @returns LocalizationConfig Localization config
     * @throws ApiError
     */
    public static getConfig(): CancelablePromise<LocalizationConfig> {
        return __request(OpenAPI, {
            method: 'GET',
            url: '/localization/config',
        });
    }
    /**
     * @returns LocalizationConfig Updated localization config
     * @throws ApiError
     */
    public static updateConfig({
        requestBody,
    }: {
        requestBody: LocalizationConfig,
    }): CancelablePromise<LocalizationConfig> {
        return __request(OpenAPI, {
            method: 'PUT',
            url: '/localization/config',
            body: requestBody,
            mediaType: 'application/json',
            errors: {
                422: `Semantic validation failure`,
            },
        });
    }
    /**
     * @returns ExternalLocalizationSource External localization sources
     * @throws ApiError
     */
    public static listExternalSources(): CancelablePromise<Array<ExternalLocalizationSource>> {
        return __request(OpenAPI, {
            method: 'GET',
            url: '/localization/external/sources',
        });
    }
    /**
     * @returns ExternalLocalizationSource Upserted external source
     * @throws ApiError
     */
    public static upsertExternalSource({
        id,
        requestBody,
    }: {
        /**
         * External source id
         */
        id: string,
        requestBody: ExternalLocalizationSourceUpsert,
    }): CancelablePromise<ExternalLocalizationSource> {
        return __request(OpenAPI, {
            method: 'PUT',
            url: '/localization/external/sources/{id}',
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
    public static deleteExternalSource({
        id,
    }: {
        /**
         * External source id
         */
        id: string,
    }): CancelablePromise<void> {
        return __request(OpenAPI, {
            method: 'DELETE',
            url: '/localization/external/sources/{id}',
            path: {
                'id': id,
            },
        });
    }
    /**
     * @returns ExternalLocalizationSample Updated sample
     * @throws ApiError
     */
    public static updateExternalSample({
        id,
        requestBody,
    }: {
        /**
         * External source id
         */
        id: string,
        requestBody: ExternalLocalizationSampleRequest,
    }): CancelablePromise<ExternalLocalizationSample> {
        return __request(OpenAPI, {
            method: 'POST',
            url: '/localization/external/sources/{id}/sample',
            path: {
                'id': id,
            },
            body: requestBody,
            mediaType: 'application/json',
        });
    }
    /**
     * @returns PipelineOutputSample Latest output sample
     * @throws ApiError
     */
    public static sampleExternalOutput({
        id,
        outputKey,
    }: {
        /**
         * External source id
         */
        id: string,
        /**
         * Output key
         */
        outputKey: string,
    }): CancelablePromise<PipelineOutputSample> {
        return __request(OpenAPI, {
            method: 'GET',
            url: '/localization/external/{id}/outputs/{output_key}',
            path: {
                'id': id,
                'output_key': outputKey,
            },
            errors: {
                400: `Unsupported output`,
                404: `No sample available`,
            },
        });
    }
    /**
     * @returns FieldMapSummary Available field maps
     * @throws ApiError
     */
    public static listMaps(): CancelablePromise<Array<FieldMapSummary>> {
        return __request(OpenAPI, {
            method: 'GET',
            url: '/localization/maps',
        });
    }
    /**
     * @returns FieldMapSummary Map uploaded
     * @throws ApiError
     */
    public static uploadLimelightFmap({
        requestBody,
    }: {
        /**
         * Multipart form-data with exactly one .fmap (JSON) file part
         */
        requestBody: string,
    }): CancelablePromise<FieldMapSummary> {
        return __request(OpenAPI, {
            method: 'POST',
            url: '/localization/maps/upload',
            body: requestBody,
            mediaType: 'text/plain',
            errors: {
                400: `Invalid upload`,
                413: `Upload too large`,
                422: `Semantic validation failure`,
            },
        });
    }
    /**
     * @returns FieldMapDocument Field map document
     * @throws ApiError
     */
    public static fetchMap({
        id,
    }: {
        /**
         * Field map id
         */
        id: string,
    }): CancelablePromise<FieldMapDocument> {
        return __request(OpenAPI, {
            method: 'GET',
            url: '/localization/maps/{id}',
            path: {
                'id': id,
            },
            errors: {
                404: `Map not found`,
            },
        });
    }
    /**
     * @returns PipelineOutputSample Latest output sample
     * @throws ApiError
     */
    public static samplePeerOutput({
        id,
        outputKey,
    }: {
        /**
         * Peer ID
         */
        id: string,
        /**
         * Output key (e.g. tag_poses)
         */
        outputKey: string,
    }): CancelablePromise<PipelineOutputSample> {
        return __request(OpenAPI, {
            method: 'GET',
            url: '/localization/peers/{id}/outputs/{output_key}',
            path: {
                'id': id,
                'output_key': outputKey,
            },
            errors: {
                400: `Unsupported output`,
                404: `No sample available`,
                502: `Peer error`,
            },
        });
    }
    /**
     * @returns LocalizationProfilesExportEnvelope Exported localization profiles envelope
     * @throws ApiError
     */
    public static exportProfiles(): CancelablePromise<LocalizationProfilesExportEnvelope> {
        return __request(OpenAPI, {
            method: 'GET',
            url: '/localization/profiles/export',
        });
    }
    /**
     * @returns LocalizationConfig Imported localization config
     * @throws ApiError
     */
    public static importProfiles({
        requestBody,
    }: {
        requestBody: LocalizationProfilesImportRequest,
    }): CancelablePromise<LocalizationConfig> {
        return __request(OpenAPI, {
            method: 'POST',
            url: '/localization/profiles/import',
            body: requestBody,
            mediaType: 'application/json',
            errors: {
                400: `Invalid import payload`,
                422: `Semantic validation failure`,
            },
        });
    }
    /**
     * @returns LocalizationSolveResponse Localization solve outputs
     * @throws ApiError
     */
    public static solve({
        profileId,
        applyFieldOrigin,
        fieldPosesOnly,
    }: {
        /**
         * Profile id override
         */
        profileId?: string,
        /**
         * Apply profile fieldOrigin transform to field-space outputs (default true)
         */
        applyFieldOrigin?: boolean,
        /**
         * Return only field-space solve poses, omitting raw detection-space outputs
         */
        fieldPosesOnly?: boolean,
    }): CancelablePromise<LocalizationSolveResponse> {
        return __request(OpenAPI, {
            method: 'GET',
            url: '/localization/solve',
            query: {
                'profile_id': profileId,
                'apply_field_origin': applyFieldOrigin,
                'field_poses_only': fieldPosesOnly,
            },
        });
    }
    /**
     * @returns LocalizationPipelineSource Available localization pipeline outputs
     * @throws ApiError
     */
    public static listSources(): CancelablePromise<Array<LocalizationPipelineSource>> {
        return __request(OpenAPI, {
            method: 'GET',
            url: '/localization/sources',
        });
    }
    /**
     * @returns PipelineOutputSample Latest output sample
     * @throws ApiError
     */
    public static sampleOutput({
        id,
        outputKey,
    }: {
        /**
         * Stream ID
         */
        id: string,
        /**
         * Graph output port
         */
        outputKey: string,
    }): CancelablePromise<PipelineOutputSample> {
        return __request(OpenAPI, {
            method: 'GET',
            url: '/localization/streams/{id}/outputs/{output_key}',
            path: {
                'id': id,
                'output_key': outputKey,
            },
            errors: {
                404: `No sample available`,
                502: `Engine error`,
            },
        });
    }
    /**
     * @returns LocalizationValidateResponse Validated + canonicalized localization config
     * @throws ApiError
     */
    public static validateLocalization({
        requestBody,
    }: {
        requestBody: LocalizationConfig,
    }): CancelablePromise<LocalizationValidateResponse> {
        return __request(OpenAPI, {
            method: 'POST',
            url: '/localization/validate',
            body: requestBody,
            mediaType: 'application/json',
            errors: {
                422: `Semantic validation failure`,
            },
        });
    }
}
