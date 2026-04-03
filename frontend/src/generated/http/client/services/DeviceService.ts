/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import type { ApplyCcmRequest } from '../models/ApplyCcmRequest';
import type { BootloaderStatus } from '../models/BootloaderStatus';
import type { BootloaderUpdateRequest } from '../models/BootloaderUpdateRequest';
import type { BootloaderUpdateResponse } from '../models/BootloaderUpdateResponse';
import type { CameraLayoutResponse } from '../models/CameraLayoutResponse';
import type { CaptureSnapshotRequest } from '../models/CaptureSnapshotRequest';
import type { CreateIdeProjectRequest } from '../models/CreateIdeProjectRequest';
import type { CreateIdeProjectResponse } from '../models/CreateIdeProjectResponse';
import type { DeviceMetricsResponse } from '../models/DeviceMetricsResponse';
import type { DeviceOperationAckResponse } from '../models/DeviceOperationAckResponse';
import type { DeviceRuntimeSnapshot } from '../models/DeviceRuntimeSnapshot';
import type { DeviceSnapshotResponse } from '../models/DeviceSnapshotResponse';
import type { DeviceSnapshotsResponse } from '../models/DeviceSnapshotsResponse';
import type { HealthPayload } from '../models/HealthPayload';
import type { HostnamePayload } from '../models/HostnamePayload';
import type { I2cInventory } from '../models/I2cInventory';
import type { IdeInfo } from '../models/IdeInfo';
import type { IdeProjectsResponse } from '../models/IdeProjectsResponse';
import type { ImuStatusPayload } from '../models/ImuStatusPayload';
import type { ImuUpdateRequest } from '../models/ImuUpdateRequest';
import type { IpaStatus } from '../models/IpaStatus';
import type { IpaTarget } from '../models/IpaTarget';
import type { LedConfig } from '../models/LedConfig';
import type { LightingAnimationListResponse } from '../models/LightingAnimationListResponse';
import type { LightingAnimationSaveRequest } from '../models/LightingAnimationSaveRequest';
import type { LightingAnimationTemplateDocument } from '../models/LightingAnimationTemplateDocument';
import type { LightingAnimationTemplateSummary } from '../models/LightingAnimationTemplateSummary';
import type { LightingCommandRequest } from '../models/LightingCommandRequest';
import type { LightingConfigRequest } from '../models/LightingConfigRequest';
import type { LightingRuntimeStatePayload } from '../models/LightingRuntimeStatePayload';
import type { LogSourcesResponse } from '../models/LogSourcesResponse';
import type { NetworkInterfaceSettings } from '../models/NetworkInterfaceSettings';
import type { Nt4Settings } from '../models/Nt4Settings';
import type { OsReleaseInfo } from '../models/OsReleaseInfo';
import type { PowerStatusPayload } from '../models/PowerStatusPayload';
import type { ResourceGuardAction } from '../models/ResourceGuardAction';
import type { ResourceGuardStatus } from '../models/ResourceGuardStatus';
import type { RestartRequest } from '../models/RestartRequest';
import type { RootStatusPayload } from '../models/RootStatusPayload';
import type { SolveCcmRequest } from '../models/SolveCcmRequest';
import type { SolveCcmResponse } from '../models/SolveCcmResponse';
import type { TeamNumberPayload } from '../models/TeamNumberPayload';
import type { UpdateCameraPoseRequest } from '../models/UpdateCameraPoseRequest';
import type { UpdateRobotDimensionsRequest } from '../models/UpdateRobotDimensionsRequest';
import type { UsbPowerSettings } from '../models/UsbPowerSettings';
import type { CancelablePromise } from '../core/CancelablePromise';
import { OpenAPI } from '../core/OpenAPI';
import { request as __request } from '../core/request';
export class DeviceService {
    /**
     * @returns RootStatusPayload Runtime status, capabilities, codec inventory, and resolved streams
     * @throws ApiError
     */
    public static rootStatus(): CancelablePromise<RootStatusPayload> {
        return __request(OpenAPI, {
            method: 'GET',
            url: '/',
        });
    }
    /**
     * @returns BootloaderStatus Bootloader firmware status
     * @throws ApiError
     */
    public static bootloaderStatus(): CancelablePromise<BootloaderStatus> {
        return __request(OpenAPI, {
            method: 'GET',
            url: '/device/bootloader',
        });
    }
    /**
     * @returns BootloaderUpdateResponse Bootloader firmware update staged
     * @throws ApiError
     */
    public static update({
        requestBody,
    }: {
        requestBody: BootloaderUpdateRequest,
    }): CancelablePromise<BootloaderUpdateResponse> {
        return __request(OpenAPI, {
            method: 'POST',
            url: '/device/bootloader/update',
            body: requestBody,
            mediaType: 'application/json',
        });
    }
    /**
     * @returns CameraLayoutResponse Camera + rig layout snapshot
     * @throws ApiError
     */
    public static getCameraLayout(): CancelablePromise<CameraLayoutResponse> {
        return __request(OpenAPI, {
            method: 'GET',
            url: '/device/camera-layout',
        });
    }
    /**
     * @returns void
     * @throws ApiError
     */
    public static updateCameraPose({
        cameraUid,
        requestBody,
    }: {
        /**
         * Camera UID (driver key)
         */
        cameraUid: string,
        requestBody: UpdateCameraPoseRequest,
    }): CancelablePromise<void> {
        return __request(OpenAPI, {
            method: 'PUT',
            url: '/device/cameras/{camera_uid}/pose',
            path: {
                'camera_uid': cameraUid,
            },
            body: requestBody,
            mediaType: 'application/json',
        });
    }
    /**
     * @returns void
     * @throws ApiError
     */
    public static clearCameraPose({
        cameraUid,
    }: {
        /**
         * Camera UID (driver key)
         */
        cameraUid: string,
    }): CancelablePromise<void> {
        return __request(OpenAPI, {
            method: 'DELETE',
            url: '/device/cameras/{camera_uid}/pose',
            path: {
                'camera_uid': cameraUid,
            },
        });
    }
    /**
     * @returns string Console placeholder
     * @throws ApiError
     */
    public static console(): CancelablePromise<Array<string>> {
        return __request(OpenAPI, {
            method: 'GET',
            url: '/device/console',
        });
    }
    /**
     * @returns HostnamePayload Device hostname
     * @throws ApiError
     */
    public static hostname(): CancelablePromise<HostnamePayload> {
        return __request(OpenAPI, {
            method: 'GET',
            url: '/device/hostname',
            errors: {
                502: `Hostname unavailable`,
            },
        });
    }
    /**
     * @returns void
     * @throws ApiError
     */
    public static setHostname({
        requestBody,
    }: {
        requestBody: HostnamePayload,
    }): CancelablePromise<void> {
        return __request(OpenAPI, {
            method: 'POST',
            url: '/device/hostname',
            body: requestBody,
            mediaType: 'application/json',
            errors: {
                400: `Invalid request`,
                502: `Hostname unavailable`,
            },
        });
    }
    /**
     * @returns I2cInventory I2C inventory
     * @throws ApiError
     */
    public static i2C(): CancelablePromise<I2cInventory> {
        return __request(OpenAPI, {
            method: 'GET',
            url: '/device/i2c',
            errors: {
                400: `Bad request`,
                503: `Peripherals IPC unavailable`,
            },
        });
    }
    /**
     * @returns IdeInfo IDE connection information
     * @throws ApiError
     */
    public static ideInfo(): CancelablePromise<IdeInfo> {
        return __request(OpenAPI, {
            method: 'GET',
            url: '/device/ide',
        });
    }
    /**
     * @returns IdeProjectsResponse IDE plugin projects
     * @throws ApiError
     */
    public static ideProjects(): CancelablePromise<IdeProjectsResponse> {
        return __request(OpenAPI, {
            method: 'GET',
            url: '/device/ide/projects',
        });
    }
    /**
     * @returns CreateIdeProjectResponse Created IDE plugin project
     * @throws ApiError
     */
    public static createIdeProject({
        requestBody,
    }: {
        requestBody: CreateIdeProjectRequest,
    }): CancelablePromise<CreateIdeProjectResponse> {
        return __request(OpenAPI, {
            method: 'POST',
            url: '/device/ide/projects',
            body: requestBody,
            mediaType: 'application/json',
        });
    }
    /**
     * @returns ImuStatusPayload IMU status
     * @throws ApiError
     */
    public static imuStatus(): CancelablePromise<ImuStatusPayload> {
        return __request(OpenAPI, {
            method: 'GET',
            url: '/device/imu',
            errors: {
                400: `Bad request`,
                503: `Peripherals IPC unavailable`,
            },
        });
    }
    /**
     * @returns ImuStatusPayload Updated IMU status
     * @throws ApiError
     */
    public static updateImu({
        requestBody,
    }: {
        requestBody: ImuUpdateRequest,
    }): CancelablePromise<ImuStatusPayload> {
        return __request(OpenAPI, {
            method: 'PATCH',
            url: '/device/imu',
            body: requestBody,
            mediaType: 'application/json',
            errors: {
                400: `Invalid request`,
                503: `Peripherals IPC unavailable`,
            },
        });
    }
    /**
     * @returns IpaStatus IPA tuning files and current CCM
     * @throws ApiError
     */
    public static ipaStatus(): CancelablePromise<IpaStatus> {
        return __request(OpenAPI, {
            method: 'GET',
            url: '/device/ipa',
        });
    }
    /**
     * @returns void
     * @throws ApiError
     */
    public static applyCcm({
        requestBody,
    }: {
        requestBody: ApplyCcmRequest,
    }): CancelablePromise<void> {
        return __request(OpenAPI, {
            method: 'POST',
            url: '/device/ipa/ccm',
            body: requestBody,
            mediaType: 'application/json',
        });
    }
    /**
     * @returns any ColorChecker Classic 24 PNG
     * @throws ApiError
     */
    public static chartPng(): CancelablePromise<any> {
        return __request(OpenAPI, {
            method: 'GET',
            url: '/device/ipa/ccm/chart',
        });
    }
    /**
     * @returns any ColorChecker Classic 24 PDF
     * @throws ApiError
     */
    public static chartPdf(): CancelablePromise<any> {
        return __request(OpenAPI, {
            method: 'GET',
            url: '/device/ipa/ccm/chart.pdf',
        });
    }
    /**
     * @returns SolveCcmResponse Solve CCM from a chart image
     * @throws ApiError
     */
    public static solveCcm({
        requestBody,
    }: {
        requestBody: SolveCcmRequest,
    }): CancelablePromise<SolveCcmResponse> {
        return __request(OpenAPI, {
            method: 'POST',
            url: '/device/ipa/ccm/solve',
            body: requestBody,
            mediaType: 'application/json',
        });
    }
    /**
     * @returns any Download IPA JSON
     * @throws ApiError
     */
    public static deviceIpaDownload({
        target,
    }: {
        target: IpaTarget,
    }): CancelablePromise<any> {
        return __request(OpenAPI, {
            method: 'GET',
            url: '/device/ipa/download',
            query: {
                'target': target,
            },
        });
    }
    /**
     * @returns void
     * @throws ApiError
     */
    public static lightingCommand({
        requestBody,
    }: {
        requestBody: LightingCommandRequest,
    }): CancelablePromise<void> {
        return __request(OpenAPI, {
            method: 'POST',
            url: '/device/lighting',
            body: requestBody,
            mediaType: 'application/json',
            errors: {
                400: `Bad request`,
                502: `Peripherals error`,
                503: `Peripherals IPC unavailable`,
            },
        });
    }
    /**
     * @returns LightingAnimationListResponse Saved lighting animations
     * @throws ApiError
     */
    public static listLightingAnimations(): CancelablePromise<LightingAnimationListResponse> {
        return __request(OpenAPI, {
            method: 'GET',
            url: '/device/lighting/animations',
        });
    }
    /**
     * @returns void
     * @throws ApiError
     */
    public static saveLightingAnimation({
        requestBody,
    }: {
        requestBody: LightingAnimationSaveRequest,
    }): CancelablePromise<void> {
        return __request(OpenAPI, {
            method: 'POST',
            url: '/device/lighting/animations',
            body: requestBody,
            mediaType: 'application/json',
            errors: {
                400: `Invalid request`,
            },
        });
    }
    /**
     * @returns void
     * @throws ApiError
     */
    public static deleteLightingAnimation({
        name,
    }: {
        name: string,
    }): CancelablePromise<void> {
        return __request(OpenAPI, {
            method: 'DELETE',
            url: '/device/lighting/animations/{name}',
            path: {
                'name': name,
            },
        });
    }
    /**
     * @returns LedConfig Lighting configuration
     * @throws ApiError
     */
    public static lightingConfig(): CancelablePromise<LedConfig> {
        return __request(OpenAPI, {
            method: 'GET',
            url: '/device/lighting/config',
        });
    }
    /**
     * @returns void
     * @throws ApiError
     */
    public static updateLightingConfig({
        requestBody,
    }: {
        requestBody: LightingConfigRequest,
    }): CancelablePromise<void> {
        return __request(OpenAPI, {
            method: 'POST',
            url: '/device/lighting/config',
            body: requestBody,
            mediaType: 'application/json',
            errors: {
                400: `Invalid request`,
            },
        });
    }
    /**
     * @returns LedConfig Lighting configuration reset
     * @throws ApiError
     */
    public static resetLightingConfig(): CancelablePromise<LedConfig> {
        return __request(OpenAPI, {
            method: 'POST',
            url: '/device/lighting/config/reset',
        });
    }
    /**
     * @returns LightingRuntimeStatePayload Current lighting runtime state
     * @throws ApiError
     */
    public static lightingState(): CancelablePromise<LightingRuntimeStatePayload> {
        return __request(OpenAPI, {
            method: 'GET',
            url: '/device/lighting/state',
            errors: {
                502: `Peripherals error`,
                503: `Peripherals IPC unavailable`,
            },
        });
    }
    /**
     * @returns LightingAnimationTemplateSummary List built-in lighting templates
     * @throws ApiError
     */
    public static listLightingTemplates(): CancelablePromise<Array<LightingAnimationTemplateSummary>> {
        return __request(OpenAPI, {
            method: 'GET',
            url: '/device/lighting/templates',
        });
    }
    /**
     * @returns LightingAnimationTemplateDocument Lighting template document
     * @throws ApiError
     */
    public static fetchLightingTemplate({
        id,
    }: {
        /**
         * Template identifier
         */
        id: string,
    }): CancelablePromise<LightingAnimationTemplateDocument> {
        return __request(OpenAPI, {
            method: 'GET',
            url: '/device/lighting/templates/{id}',
            path: {
                'id': id,
            },
            errors: {
                404: `Template not found`,
            },
        });
    }
    /**
     * @returns string Logs placeholder
     * @throws ApiError
     */
    public static logs(): CancelablePromise<Array<string>> {
        return __request(OpenAPI, {
            method: 'GET',
            url: '/device/logs',
        });
    }
    /**
     * @returns any Log download
     * @throws ApiError
     */
    public static deviceLogsDownload({
        source,
        lines,
    }: {
        /**
         * Log source ID
         */
        source?: string,
        /**
         * Optional line limit; omit for full output
         */
        lines?: number,
    }): CancelablePromise<any> {
        return __request(OpenAPI, {
            method: 'GET',
            url: '/device/logs/download',
            query: {
                'source': source,
                'lines': lines,
            },
        });
    }
    /**
     * @returns LogSourcesResponse Available log sources
     * @throws ApiError
     */
    public static sources(): CancelablePromise<LogSourcesResponse> {
        return __request(OpenAPI, {
            method: 'GET',
            url: '/device/logs/sources',
        });
    }
    /**
     * @returns DeviceMetricsResponse Device metrics
     * @throws ApiError
     */
    public static metrics(): CancelablePromise<DeviceMetricsResponse> {
        return __request(OpenAPI, {
            method: 'GET',
            url: '/device/metrics',
        });
    }
    /**
     * @returns NetworkInterfaceSettings Network config
     * @throws ApiError
     */
    public static network(): CancelablePromise<Array<NetworkInterfaceSettings>> {
        return __request(OpenAPI, {
            method: 'GET',
            url: '/device/network',
            errors: {
                502: `Network unavailable`,
            },
        });
    }
    /**
     * @returns void
     * @throws ApiError
     */
    public static setNetwork({
        requestBody,
    }: {
        requestBody: NetworkInterfaceSettings,
    }): CancelablePromise<void> {
        return __request(OpenAPI, {
            method: 'POST',
            url: '/device/network',
            body: requestBody,
            mediaType: 'application/json',
            errors: {
                400: `Invalid request`,
            },
        });
    }
    /**
     * @returns Nt4Settings NT4 settings
     * @throws ApiError
     */
    public static getNt4Settings(): CancelablePromise<Nt4Settings> {
        return __request(OpenAPI, {
            method: 'GET',
            url: '/device/nt4',
        });
    }
    /**
     * @returns void
     * @throws ApiError
     */
    public static setNt4Settings({
        requestBody,
    }: {
        requestBody: Nt4Settings,
    }): CancelablePromise<void> {
        return __request(OpenAPI, {
            method: 'POST',
            url: '/device/nt4',
            body: requestBody,
            mediaType: 'application/json',
            errors: {
                400: `Invalid request`,
            },
        });
    }
    /**
     * @returns OsReleaseInfo OS release info
     * @throws ApiError
     */
    public static osRelease(): CancelablePromise<OsReleaseInfo> {
        return __request(OpenAPI, {
            method: 'GET',
            url: '/device/os',
            errors: {
                404: `OS release not found`,
            },
        });
    }
    /**
     * @returns PowerStatusPayload Power status
     * @throws ApiError
     */
    public static powerStatus(): CancelablePromise<PowerStatusPayload> {
        return __request(OpenAPI, {
            method: 'GET',
            url: '/device/power',
            errors: {
                400: `Bad request`,
                503: `Peripherals IPC unavailable`,
            },
        });
    }
    /**
     * @returns ResourceGuardStatus Resource guard status
     * @throws ApiError
     */
    public static resourceGuardStatus(): CancelablePromise<ResourceGuardStatus> {
        return __request(OpenAPI, {
            method: 'GET',
            url: '/device/resource-guard',
        });
    }
    /**
     * @returns ResourceGuardAction Resource guard stream restore action
     * @throws ApiError
     */
    public static restore({
        streamId,
    }: {
        /**
         * Stream ID
         */
        streamId: string,
    }): CancelablePromise<ResourceGuardAction> {
        return __request(OpenAPI, {
            method: 'POST',
            url: '/device/resource-guard/restore/{stream_id}',
            path: {
                'stream_id': streamId,
            },
        });
    }
    /**
     * @returns DeviceOperationAckResponse Restart queued
     * @throws ApiError
     */
    public static restart({
        requestBody,
    }: {
        requestBody: RestartRequest,
    }): CancelablePromise<DeviceOperationAckResponse> {
        return __request(OpenAPI, {
            method: 'POST',
            url: '/device/restart',
            body: requestBody,
            mediaType: 'application/json',
            errors: {
                502: `systemctl failed`,
            },
        });
    }
    /**
     * @returns CameraLayoutResponse Updated camera layout snapshot
     * @throws ApiError
     */
    public static updateRobotDimensions({
        requestBody,
    }: {
        requestBody: UpdateRobotDimensionsRequest,
    }): CancelablePromise<CameraLayoutResponse> {
        return __request(OpenAPI, {
            method: 'PATCH',
            url: '/device/robot-dimensions',
            body: requestBody,
            mediaType: 'application/json',
        });
    }
    /**
     * @returns DeviceRuntimeSnapshot Platform capabilities, resolved runtime policy, and observability snapshot
     * @throws ApiError
     */
    public static deviceRuntime(): CancelablePromise<DeviceRuntimeSnapshot> {
        return __request(OpenAPI, {
            method: 'GET',
            url: '/device/runtime',
        });
    }
    /**
     * @returns DeviceSnapshotsResponse List diagnostics snapshots
     * @throws ApiError
     */
    public static deviceSnapshotsList(): CancelablePromise<DeviceSnapshotsResponse> {
        return __request(OpenAPI, {
            method: 'GET',
            url: '/device/snapshots',
        });
    }
    /**
     * @returns DeviceSnapshotResponse Snapshot captured
     * @throws ApiError
     */
    public static deviceSnapshotsCapture({
        requestBody,
    }: {
        requestBody: CaptureSnapshotRequest,
    }): CancelablePromise<DeviceSnapshotResponse> {
        return __request(OpenAPI, {
            method: 'POST',
            url: '/device/snapshots',
            body: requestBody,
            mediaType: 'application/json',
        });
    }
    /**
     * @returns void
     * @throws ApiError
     */
    public static deviceSnapshotsDelete({
        id,
    }: {
        /**
         * Snapshot ID
         */
        id: string,
    }): CancelablePromise<void> {
        return __request(OpenAPI, {
            method: 'DELETE',
            url: '/device/snapshots/{id}',
            path: {
                'id': id,
            },
        });
    }
    /**
     * @returns any Snapshot archive
     * @throws ApiError
     */
    public static deviceSnapshotsDownload({
        id,
    }: {
        /**
         * Snapshot ID
         */
        id: string,
    }): CancelablePromise<any> {
        return __request(OpenAPI, {
            method: 'GET',
            url: '/device/snapshots/{id}/download',
            path: {
                'id': id,
            },
        });
    }
    /**
     * @returns TeamNumberPayload Team number
     * @throws ApiError
     */
    public static team(): CancelablePromise<TeamNumberPayload> {
        return __request(OpenAPI, {
            method: 'GET',
            url: '/device/team',
        });
    }
    /**
     * @returns void
     * @throws ApiError
     */
    public static setTeam({
        requestBody,
    }: {
        requestBody: TeamNumberPayload,
    }): CancelablePromise<void> {
        return __request(OpenAPI, {
            method: 'POST',
            url: '/device/team',
            body: requestBody,
            mediaType: 'application/json',
            errors: {
                400: `Invalid request`,
            },
        });
    }
    /**
     * @returns UsbPowerSettings USB power settings
     * @throws ApiError
     */
    public static getUsbPowerSettings(): CancelablePromise<UsbPowerSettings> {
        return __request(OpenAPI, {
            method: 'GET',
            url: '/device/usb-power',
        });
    }
    /**
     * @returns void
     * @throws ApiError
     */
    public static setUsbPowerSettings({
        requestBody,
    }: {
        requestBody: UsbPowerSettings,
    }): CancelablePromise<void> {
        return __request(OpenAPI, {
            method: 'POST',
            url: '/device/usb-power',
            body: requestBody,
            mediaType: 'application/json',
            errors: {
                400: `Invalid request`,
                502: `USB power command failed`,
                503: `USB power control unavailable`,
            },
        });
    }
    /**
     * @returns HealthPayload Backend is reachable
     * @throws ApiError
     */
    public static health(): CancelablePromise<HealthPayload> {
        return __request(OpenAPI, {
            method: 'GET',
            url: '/health',
        });
    }
}
