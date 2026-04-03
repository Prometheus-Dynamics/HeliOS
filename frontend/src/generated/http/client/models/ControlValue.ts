/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
/**
 * Control value variants with minimal footprint.
 *
 * # Example
 * ```rust
 * use styx_core::prelude::ControlValue;
 *
 * let v = ControlValue::Bool(true);
 * assert_eq!(v, ControlValue::Bool(true));
 * ```
 */
export type ControlValue = ('None' | {
    /**
     * Boolean value.
     */
    Bool: boolean;
} | {
    /**
     * Signed integer.
     */
    Int: number;
} | {
    /**
     * Unsigned integer.
     */
    Uint: number;
} | {
    /**
     * Floating-point value.
     */
    Float: number;
});

