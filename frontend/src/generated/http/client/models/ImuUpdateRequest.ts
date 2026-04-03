/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
export type ImuUpdateRequest = {
    dr_lock_position?: boolean | null;
    dr_max_accel_world_mps2?: number | null;
    dr_max_position_m?: number | null;
    dr_max_speed_mps?: number | null;
    dr_still_velocity_zero_tau_seconds?: number | null;
    dr_velocity_damp_tau_seconds?: number | null;
    fusion?: string | null;
    /**
     * Align the current acceleration (gravity) direction to a chassis axis (e.g. "+y", "-z").
     */
    gravity_reference_axis?: string | null;
    range?: string | null;
    /**
     * Reset integrated world-frame velocity/position used for short-window dead reckoning.
     */
    reset_pose?: boolean | null;
    /**
     * Convenience flag: align gravity to the nearest axis.
     */
    snap_gravity?: boolean | null;
    update_interval_ms?: number | null;
};

