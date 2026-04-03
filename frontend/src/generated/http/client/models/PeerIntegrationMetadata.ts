/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import type { PeerCustomIntegrationConfig } from './PeerCustomIntegrationConfig';
import type { PeerIntegrationKind } from './PeerIntegrationKind';
import type { PeerIntegrationPose } from './PeerIntegrationPose';
export type PeerIntegrationMetadata = {
    camera_pose?: (null | PeerIntegrationPose);
    custom?: (null | PeerCustomIntegrationConfig);
    kind?: PeerIntegrationKind;
    localization_outputs?: Array<string>;
    management_url?: string | null;
    stream_url?: string | null;
    stream_urls?: Array<string>;
};

