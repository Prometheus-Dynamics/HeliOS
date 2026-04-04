/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import type { ApiHardwareReadModelPolicySnapshot } from './ApiHardwareReadModelPolicySnapshot';
import type { ApiStreamsPolicySnapshot } from './ApiStreamsPolicySnapshot';
import type { ApiSystemReadModelPolicySnapshot } from './ApiSystemReadModelPolicySnapshot';
import type { EngineCrashGuardPolicySnapshot } from './EngineCrashGuardPolicySnapshot';
import type { I2cInventoryPolicySnapshot } from './I2cInventoryPolicySnapshot';
import type { ImuRuntimePolicySnapshot } from './ImuRuntimePolicySnapshot';
import type { LogSourcesPolicySnapshot } from './LogSourcesPolicySnapshot';
import type { Nt4SettingsCachePolicySnapshot } from './Nt4SettingsCachePolicySnapshot';
import type { PeripheralsPowerPolicySnapshot } from './PeripheralsPowerPolicySnapshot';
import type { ResourceGuardPolicySnapshot } from './ResourceGuardPolicySnapshot';
import type { StartupCacheWarmPolicySnapshot } from './StartupCacheWarmPolicySnapshot';
import type { StyxCaptureTunablesSnapshot } from './StyxCaptureTunablesSnapshot';
import type { TokioRuntimePolicySnapshot } from './TokioRuntimePolicySnapshot';
export type DeviceRuntimePoliciesSnapshot = {
    api_hardware_read_model: ApiHardwareReadModelPolicySnapshot;
    api_streams: ApiStreamsPolicySnapshot;
    api_system_read_model: ApiSystemReadModelPolicySnapshot;
    api_tokio: TokioRuntimePolicySnapshot;
    engine_crash_guard: EngineCrashGuardPolicySnapshot;
    engine_tokio: TokioRuntimePolicySnapshot;
    i2c_inventory: I2cInventoryPolicySnapshot;
    imu: ImuRuntimePolicySnapshot;
    log_filter: string;
    log_sources: LogSourcesPolicySnapshot;
    nt4_settings_cache: Nt4SettingsCachePolicySnapshot;
    peripherals_power: PeripheralsPowerPolicySnapshot;
    peripherals_tokio: TokioRuntimePolicySnapshot;
    resource_guard: ResourceGuardPolicySnapshot;
    startup_cache_warm: StartupCacheWarmPolicySnapshot;
    styx_capture: StyxCaptureTunablesSnapshot;
};

