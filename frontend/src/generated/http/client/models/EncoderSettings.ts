/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import type { FrameRate } from './FrameRate';
import type { ResolutionHint } from './ResolutionHint';
export type EncoderSettings = ({
    kind: 'turbojpeg';
    quality?: number | null;
} | {
    kind: 'mozjpeg';
    quality?: number | null;
} | {
    bitrate?: number | null;
    framerate?: (null | FrameRate);
    gop?: number | null;
    kind: 'ffmpeg_mjpeg';
    output_resolution?: (null | ResolutionHint);
    thread_count?: number | null;
} | {
    bitrate?: number | null;
    framerate?: (null | FrameRate);
    gop?: number | null;
    kind: 'h264';
    output_resolution?: (null | ResolutionHint);
    thread_count?: number | null;
} | {
    bitrate?: number | null;
    framerate?: (null | FrameRate);
    gop?: number | null;
    kind: 'h265';
    output_resolution?: (null | ResolutionHint);
    thread_count?: number | null;
});

