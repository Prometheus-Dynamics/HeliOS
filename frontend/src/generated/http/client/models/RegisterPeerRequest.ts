/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import type { PeerEndpoint } from './PeerEndpoint';
import type { PeerIntegrationMetadata } from './PeerIntegrationMetadata';
export type RegisterPeerRequest = {
    alias?: string | null;
    api_base_url?: string | null;
    capabilities?: Array<string>;
    device_ip?: string | null;
    endpoints?: Array<PeerEndpoint>;
    integration?: (null | PeerIntegrationMetadata);
    peer_id?: string | null;
    version?: string | null;
};

