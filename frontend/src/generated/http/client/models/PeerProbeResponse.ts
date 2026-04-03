/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import type { Nt4PeerProbe } from './Nt4PeerProbe';
import type { PeerIntegrationKind } from './PeerIntegrationKind';
import type { PhotonvisionDiscoverStreamsResponse } from './PhotonvisionDiscoverStreamsResponse';
import type { ProbeResult } from './ProbeResult';
export type PeerProbeResponse = {
    api?: (null | ProbeResult);
    kind: PeerIntegrationKind;
    management?: (null | ProbeResult);
    nt4?: (null | Nt4PeerProbe);
    photonvision?: (null | PhotonvisionDiscoverStreamsResponse);
    stream?: (null | ProbeResult);
    streams?: Array<ProbeResult>;
};

