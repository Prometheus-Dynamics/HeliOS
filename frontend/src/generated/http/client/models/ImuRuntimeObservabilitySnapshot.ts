/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
export type ImuRuntimeObservabilitySnapshot = {
    accel_gyro_source?: string | null;
    angular_speed_dps?: number | null;
    available: boolean;
    dr_confidence?: number | null;
    fusion?: string | null;
    is_moving?: boolean | null;
    is_still?: boolean | null;
    last_error?: string | null;
    linear_speed_mps?: number | null;
    magnetometer_source?: string | null;
    motion_fast_g?: number | null;
    motion_g?: number | null;
    stillness_confidence?: number | null;
    update_interval_ms?: number | null;
    updated_at?: string | null;
};

