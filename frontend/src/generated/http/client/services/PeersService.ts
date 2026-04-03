/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import type { PeerDiscoveryRequest } from '../models/PeerDiscoveryRequest';
import type { PeerDiscoveryResponse } from '../models/PeerDiscoveryResponse';
import type { PeerInventoryResponse } from '../models/PeerInventoryResponse';
import type { PeerPipelineSyncRequest } from '../models/PeerPipelineSyncRequest';
import type { PeerPipelineSyncResponse } from '../models/PeerPipelineSyncResponse';
import type { PeerProbeRequest } from '../models/PeerProbeRequest';
import type { PeerProbeResponse } from '../models/PeerProbeResponse';
import type { PeerRegistrationResponse } from '../models/PeerRegistrationResponse';
import type { PeerRemoteStreamsResponse } from '../models/PeerRemoteStreamsResponse';
import type { PeerRemovalResponse } from '../models/PeerRemovalResponse';
import type { PhotonvisionDiscoverStreamsRequest } from '../models/PhotonvisionDiscoverStreamsRequest';
import type { PhotonvisionDiscoverStreamsResponse } from '../models/PhotonvisionDiscoverStreamsResponse';
import type { RegisterPeerRequest } from '../models/RegisterPeerRequest';
import type { CancelablePromise } from '../core/CancelablePromise';
import { OpenAPI } from '../core/OpenAPI';
import { request as __request } from '../core/request';
export class PeersService {
    /**
     * @returns PeerInventoryResponse Known peers
     * @throws ApiError
     */
    public static listPeers(): CancelablePromise<PeerInventoryResponse> {
        return __request(OpenAPI, {
            method: 'GET',
            url: '/peers',
        });
    }
    /**
     * @returns PeerRegistrationResponse Peer registered
     * @throws ApiError
     */
    public static registerPeer({
        requestBody,
    }: {
        requestBody: RegisterPeerRequest,
    }): CancelablePromise<PeerRegistrationResponse> {
        return __request(OpenAPI, {
            method: 'POST',
            url: '/peers',
            body: requestBody,
            mediaType: 'application/json',
            errors: {
                400: `Invalid payload`,
            },
        });
    }
    /**
     * @returns PeerDiscoveryResponse Discovery scheduled
     * @throws ApiError
     */
    public static discoverPeers({
        requestBody,
    }: {
        requestBody: PeerDiscoveryRequest,
    }): CancelablePromise<PeerDiscoveryResponse> {
        return __request(OpenAPI, {
            method: 'POST',
            url: '/peers/discover',
            body: requestBody,
            mediaType: 'application/json',
        });
    }
    /**
     * @returns PhotonvisionDiscoverStreamsResponse Discovered streams
     * @throws ApiError
     */
    public static photonvisionDiscoverStreams({
        requestBody,
    }: {
        requestBody: PhotonvisionDiscoverStreamsRequest,
    }): CancelablePromise<PhotonvisionDiscoverStreamsResponse> {
        return __request(OpenAPI, {
            method: 'POST',
            url: '/peers/integrations/photonvision/streams',
            body: requestBody,
            mediaType: 'application/json',
            errors: {
                400: `Invalid payload`,
            },
        });
    }
    /**
     * @returns PeerProbeResponse Probe result
     * @throws ApiError
     */
    public static probePeer({
        requestBody,
    }: {
        requestBody: PeerProbeRequest,
    }): CancelablePromise<PeerProbeResponse> {
        return __request(OpenAPI, {
            method: 'POST',
            url: '/peers/probe',
            body: requestBody,
            mediaType: 'application/json',
            errors: {
                400: `Invalid payload`,
            },
        });
    }
    /**
     * @returns PeerRemoteStreamsResponse Aggregated remote Helios stream inventory
     * @throws ApiError
     */
    public static listPeerStreams(): CancelablePromise<PeerRemoteStreamsResponse> {
        return __request(OpenAPI, {
            method: 'GET',
            url: '/peers/streams',
        });
    }
    /**
     * @returns PeerRemovalResponse Peer removed
     * @throws ApiError
     */
    public static removePeer({
        id,
    }: {
        /**
         * Peer identifier
         */
        id: string,
    }): CancelablePromise<PeerRemovalResponse> {
        return __request(OpenAPI, {
            method: 'DELETE',
            url: '/peers/{id}',
            path: {
                'id': id,
            },
            errors: {
                404: `Peer not found`,
            },
        });
    }
    /**
     * @returns PeerPipelineSyncResponse Peer pipelines synchronized into local storage
     * @throws ApiError
     */
    public static syncPeerPipelines({
        id,
        requestBody,
    }: {
        /**
         * Peer identifier
         */
        id: string,
        requestBody: PeerPipelineSyncRequest,
    }): CancelablePromise<PeerPipelineSyncResponse> {
        return __request(OpenAPI, {
            method: 'POST',
            url: '/peers/{id}/pipelines/sync',
            path: {
                'id': id,
            },
            body: requestBody,
            mediaType: 'application/json',
            errors: {
                400: `Unsupported peer type`,
                404: `Peer not found`,
            },
        });
    }
    /**
     * @returns PeerRemoteStreamsResponse Remote Helios stream inventory for one peer
     * @throws ApiError
     */
    public static listPeerStreamsForPeer({
        id,
    }: {
        /**
         * Peer identifier
         */
        id: string,
    }): CancelablePromise<PeerRemoteStreamsResponse> {
        return __request(OpenAPI, {
            method: 'GET',
            url: '/peers/{id}/streams',
            path: {
                'id': id,
            },
            errors: {
                400: `Unsupported peer type`,
                404: `Peer not found`,
            },
        });
    }
}
