/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import type { FanStatus } from './FanStatus';
import type { I2cInventory } from './I2cInventory';
import type { LightingStatus } from './LightingStatus';
import type { PeripheralErrors } from './PeripheralErrors';
import type { ProbedDevice } from './ProbedDevice';
import type { SensorPeripheral } from './SensorPeripheral';
import type { UsbPeripheral } from './UsbPeripheral';
export type PeripheralInventory = {
    cameras: Array<ProbedDevice>;
    errors?: PeripheralErrors;
    fan?: (null | FanStatus);
    i2c?: (null | I2cInventory);
    lighting?: (null | LightingStatus);
    sensors?: Array<SensorPeripheral>;
    usb?: Array<UsbPeripheral>;
};

