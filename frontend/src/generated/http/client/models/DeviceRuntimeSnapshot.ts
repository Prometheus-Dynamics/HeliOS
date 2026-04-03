/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import type { DeviceCapabilitySnapshot } from './DeviceCapabilitySnapshot';
import type { DeviceRuntimeObservabilitySnapshot } from './DeviceRuntimeObservabilitySnapshot';
import type { DeviceRuntimePoliciesSnapshot } from './DeviceRuntimePoliciesSnapshot';
import type { PlatformIdentityPayload } from './PlatformIdentityPayload';
export type DeviceRuntimeSnapshot = {
    capabilities: DeviceCapabilitySnapshot;
    observability: DeviceRuntimeObservabilitySnapshot;
    platform: PlatformIdentityPayload;
    policies: DeviceRuntimePoliciesSnapshot;
};

