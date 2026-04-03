/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import type { PeerDiscoveryResponse } from './PeerDiscoveryResponse';
import type { PeerInfo } from './PeerInfo';
export type PeerInventoryResponse = {
    discovery?: (null | PeerDiscoveryResponse);
    peers?: Array<PeerInfo>;
};

