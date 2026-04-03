/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
export type Nt4Settings = {
    /**
     * Enable Limelight-compatible API emulation (per-stream adapters).
     */
    emulate_limelight_api?: boolean;
    /**
     * Enable PhotonVision-compatible API emulation (device-wide adapter).
     */
    emulate_photonvision_api?: boolean;
    enabled?: boolean;
    public_api_url?: string | null;
    server_host?: string | null;
    server_port?: number | null;
    /**
     * When disabled, HeliOS will avoid subscribing to NetworkTables topics (used by explorer + peer telemetry).
     */
    subscriptions_enabled?: boolean;
};

