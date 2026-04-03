/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
/**
 * Physical device identity derived from fingerprints/props.
 *
 * `display` is a human-friendly string, while `keys` contains fingerprints
 * that help merge identical devices across backends.
 */
export type DeviceIdentity = {
    /**
     * Display-friendly identifier.
     */
    display: string;
    /**
     * Fingerprint keys used for matching.
     */
    keys: Array<string>;
};

