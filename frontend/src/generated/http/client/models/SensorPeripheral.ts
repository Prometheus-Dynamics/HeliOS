/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import type { SensorPeripheralFirmwareStatus } from './SensorPeripheralFirmwareStatus';
import type { SensorPeripheralWarning } from './SensorPeripheralWarning';
export type SensorPeripheral = {
    alias?: string | null;
    alias_identity?: string | null;
    driver_camera_id: string;
    driver_namespace: string;
    firmware?: (null | SensorPeripheralFirmwareStatus);
    hardware_id?: string | null;
    hardware_key?: string | null;
    interval?: string | null;
    name: string;
    present?: boolean;
    status?: string | null;
    telemetry?: any | null;
    type?: string | null;
    warnings?: Array<SensorPeripheralWarning>;
};

