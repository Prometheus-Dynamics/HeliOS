/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import type { FanCurvePoint } from './FanCurvePoint';
/**
 * Declarative configuration for a PWM fan curve.
 */
export type FanConfig = {
    /**
     * Sorted temperature breakpoints and corresponding duty cycles.
     */
    curve?: Array<FanCurvePoint>;
    enabled?: boolean;
    /**
     * Whether PWM polarity should be inverted (100% duty = 0% fan speed).
     */
    invert_pwm?: boolean;
    /**
     * Optional manual override; when set, the controller drives the fan at this percentage.
     */
    manual_percent?: number | null;
    /**
     * Maximum duty cycle expressed as a percentage (0-100).
     */
    max_percent?: number;
    /**
     * Minimum duty cycle expressed as a percentage (0-100).
     */
    min_percent?: number;
    /**
     * Poll interval for recomputing the fan target, in milliseconds.
     */
    poll_interval_ms?: number;
    /**
     * Sysfs path to the pwmN file exposed by the fan driver (usually under /sys/class/hwmon).
     */
    pwm_path?: string;
    /**
     * Optional sysfs path that reports tachometer readings (fan*_input) in RPM.
     */
    tacho_path?: string | null;
};

