/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import type { EncoderSettings } from './EncoderSettings';
export type RequestedEncoderConfigSchema = ({
    state: 'disabled';
} | {
    id?: string | null;
    settings?: (null | EncoderSettings);
    state: 'enabled';
});

