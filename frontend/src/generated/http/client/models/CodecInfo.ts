/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import type { CodecKind } from './CodecKind';
import type { CodecTunables } from './CodecTunables';
export type CodecInfo = {
    fourcc: string;
    implementation: string;
    input: string;
    kind: CodecKind;
    name: string;
    output: string;
    tunables?: (null | CodecTunables);
};

