/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import type { PeerEndpoint } from './PeerEndpoint';
import type { PeerIntegrationMetadata } from './PeerIntegrationMetadata';
import type { PeerStatus } from './PeerStatus';
export type PeerInfo = {
    alias?: string | null;
    api_base_url: string;
    capabilities?: Array<string>;
    endpoints?: Array<PeerEndpoint>;
    id: string;
    integration: PeerIntegrationMetadata;
    last_seen_at?: string | null;
    latency_ms?: number | null;
    status: PeerStatus;
    telemetry?: Record<string, any>;
    version?: string | null;
};

