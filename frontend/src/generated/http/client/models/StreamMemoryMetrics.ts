/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import type { StreamBufferPoolMetrics } from './StreamBufferPoolMetrics';
import type { StreamExternalBackingMetrics } from './StreamExternalBackingMetrics';
import type { StreamPackedPoolMetrics } from './StreamPackedPoolMetrics';
import type { StreamProcessMemoryMetrics } from './StreamProcessMemoryMetrics';
import type { StreamQueueMemoryMetrics } from './StreamQueueMemoryMetrics';
import type { StreamRunnerMemoryMetrics } from './StreamRunnerMemoryMetrics';
import type { StreamStagingCopyMetrics } from './StreamStagingCopyMetrics';
export type StreamMemoryMetrics = {
    capture_queue?: (null | StreamQueueMemoryMetrics);
    external_backings?: Array<StreamExternalBackingMetrics>;
    image_pool?: (null | StreamBufferPoolMetrics);
    packed_pools?: Array<StreamPackedPoolMetrics>;
    process?: (null | StreamProcessMemoryMetrics);
    runner?: (null | StreamRunnerMemoryMetrics);
    staging_copy?: (null | StreamStagingCopyMetrics);
    transform_pool?: (null | StreamBufferPoolMetrics);
};

