/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import type { DaedalusRegistryNode } from './DaedalusRegistryNode';
import type { DaedalusRegistryType } from './DaedalusRegistryType';
export type DaedalusRegistryResponse = {
    nodes: Array<DaedalusRegistryNode>;
    plugins: Array<string>;
    types?: Array<DaedalusRegistryType>;
};

