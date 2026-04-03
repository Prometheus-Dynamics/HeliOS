/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import type { PeerRemoteStreamSummary } from './PeerRemoteStreamSummary';
import type { PeerResourceError } from './PeerResourceError';
export type PeerRemoteStreamsResponse = {
    errors?: Array<PeerResourceError>;
    fetched_at: string;
    streams?: Array<PeerRemoteStreamSummary>;
};

