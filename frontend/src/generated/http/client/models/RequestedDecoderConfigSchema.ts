/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import type { DecoderSettings } from './DecoderSettings';
export type RequestedDecoderConfigSchema = ({
    state: 'disabled';
} | {
    id?: string | null;
    settings?: (null | DecoderSettings);
    state: 'enabled';
});

