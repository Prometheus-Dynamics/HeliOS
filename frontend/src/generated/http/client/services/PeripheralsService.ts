/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import type { CameraDiscoveryResponse } from '../models/CameraDiscoveryResponse';
import type { FanStatus } from '../models/FanStatus';
import type { I2cInventory } from '../models/I2cInventory';
import type { LightingStatus } from '../models/LightingStatus';
import type { PeripheralInventory } from '../models/PeripheralInventory';
import type { UsbPeripheral } from '../models/UsbPeripheral';
import type { CancelablePromise } from '../core/CancelablePromise';
import { OpenAPI } from '../core/OpenAPI';
import { request as __request } from '../core/request';
export class PeripheralsService {
    /**
     * @returns PeripheralInventory Peripherals and cameras
     * @throws ApiError
     */
    public static listPeripherals(): CancelablePromise<PeripheralInventory> {
        return __request(OpenAPI, {
            method: 'GET',
            url: '/peripherals',
            errors: {
                502: `Peripheral error`,
            },
        });
    }
    /**
     * @returns CameraDiscoveryResponse Discovered cameras
     * @throws ApiError
     */
    public static listCameras(): CancelablePromise<CameraDiscoveryResponse> {
        return __request(OpenAPI, {
            method: 'GET',
            url: '/peripherals/cameras',
            errors: {
                502: `Camera discovery failed`,
            },
        });
    }
    /**
     * @returns FanStatus Fan status
     * @throws ApiError
     */
    public static fanStatus(): CancelablePromise<FanStatus> {
        return __request(OpenAPI, {
            method: 'GET',
            url: '/peripherals/fan',
            errors: {
                503: `Peripherals IPC unavailable`,
            },
        });
    }
    /**
     * @returns I2cInventory I2C inventory
     * @throws ApiError
     */
    public static listI2C(): CancelablePromise<I2cInventory> {
        return __request(OpenAPI, {
            method: 'GET',
            url: '/peripherals/i2c',
            errors: {
                400: `Bad request`,
                503: `Peripherals IPC unavailable`,
            },
        });
    }
    /**
     * @returns LightingStatus Lighting presence
     * @throws ApiError
     */
    public static ledStatus(): CancelablePromise<LightingStatus> {
        return __request(OpenAPI, {
            method: 'GET',
            url: '/peripherals/leds',
        });
    }
    /**
     * @returns UsbPeripheral USB peripherals
     * @throws ApiError
     */
    public static listUsb(): CancelablePromise<Array<UsbPeripheral>> {
        return __request(OpenAPI, {
            method: 'GET',
            url: '/peripherals/usb',
        });
    }
}
