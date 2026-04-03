/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import type { DaedalusRegistryFanInPort } from './DaedalusRegistryFanInPort';
import type { DaedalusRegistryPort } from './DaedalusRegistryPort';
import type { DaedalusSyncGroup } from './DaedalusSyncGroup';
export type DaedalusRegistryNode = {
    default_compute: string;
    fanin_inputs?: Array<DaedalusRegistryFanInPort>;
    feature_flags: Array<string>;
    id: string;
    input_ports?: Array<DaedalusRegistryPort>;
    inputs: Array<string>;
    label?: string | null;
    metadata: Record<string, any>;
    output_ports?: Array<DaedalusRegistryPort>;
    outputs: Array<string>;
    plugin?: string | null;
    sync_groups: Array<DaedalusSyncGroup>;
};

