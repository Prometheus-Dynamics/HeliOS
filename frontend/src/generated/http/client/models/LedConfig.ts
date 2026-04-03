/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
/**
 * Declarative configuration for a single addressable LED chain.
 */
export type LedConfig = {
    brightness?: number | null;
    color_order?: string;
    count?: number;
    default_animations?: Record<string, string>;
    enabled?: boolean;
    frequency_hz?: number;
    gpio?: number;
    label?: string | null;
    protocol?: string;
    use_pwm?: boolean;
};

