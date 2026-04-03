/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
export type DecoderSettings = {
    /**
     * Optional soft limit for decode FPS; frames above this are dropped before decode/graph.
     */
    fps_limit?: number | null;
    /**
     * Optional horizontal mirror applied after decode.
     */
    mirror_horizontal?: boolean | null;
    /**
     * Optional rotation applied after decode (0/90/180/270 degrees).
     */
    rotation_degrees?: number | null;
    /**
     * Optional decoder thread count (codec-dependent).
     */
    thread_count?: number | null;
};

