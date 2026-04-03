/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import type { PeerPipelineSyncItem } from './PeerPipelineSyncItem';
export type PeerPipelineSyncResponse = {
    errors?: Array<string>;
    peer_alias?: string | null;
    peer_id: string;
    synced?: Array<PeerPipelineSyncItem>;
};

