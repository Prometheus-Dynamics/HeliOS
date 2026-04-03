/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import type { PeerIntegrationKind } from './PeerIntegrationKind';
export type PeerProbeRequest = {
    api_base_url?: string | null;
    device_ip?: string | null;
    kind: PeerIntegrationKind;
    management_url?: string | null;
    network_table?: string | null;
    stream_url?: string | null;
    stream_urls?: Array<string>;
    timeout_ms?: number | null;
};

