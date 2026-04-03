/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import type { ImuAxesPayload } from './ImuAxesPayload';
import type { ImuOptionsPayload } from './ImuOptionsPayload';
import type { ImuOrientationPayload } from './ImuOrientationPayload';
import type { ImuSourcesPayload } from './ImuSourcesPayload';
export type ImuStatusPayload = {
    accel?: (null | ImuAxesPayload);
    angular_speed_dps?: number | null;
    angular_speed_normalized?: number | null;
    angular_velocity_dps?: (null | ImuAxesPayload);
    corrected_world_accel_mps2?: (null | ImuAxesPayload);
    dr_confidence?: number | null;
    dr_lock_position?: boolean | null;
    dr_max_accel_world_mps2?: number | null;
    dr_max_position_m?: number | null;
    dr_max_speed_mps?: number | null;
    dr_still_velocity_zero_tau_seconds?: number | null;
    dr_velocity_damp_tau_seconds?: number | null;
    dt_seconds?: number | null;
    fusion?: string | null;
    gyro?: (null | ImuAxesPayload);
    gyro_bias_dps?: (null | ImuAxesPayload);
    has_sample?: boolean;
    is_moving?: boolean | null;
    is_moving_fast?: boolean | null;
    is_still?: boolean | null;
    last_error?: string | null;
    linear_accel?: (null | ImuAxesPayload);
    linear_speed_mps?: number | null;
    linear_speed_normalized?: number | null;
    mag?: (null | ImuAxesPayload);
    motion_fast_g?: number | null;
    motion_fast_threshold_g?: number | null;
    motion_g?: number | null;
    motion_noise_floor_g?: number | null;
    options?: (null | ImuOptionsPayload);
    orientation?: (null | ImuOrientationPayload);
    position_world?: (null | ImuAxesPayload);
    range?: string | null;
    rotation_contaminated?: boolean | null;
    sources?: (null | ImuSourcesPayload);
    stillness_confidence?: number | null;
    update_interval_ms?: number | null;
    updated_at?: string | null;
    velocity_delta_world?: (null | ImuAxesPayload);
    velocity_world?: (null | ImuAxesPayload);
};

