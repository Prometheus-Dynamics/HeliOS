/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import type { ApiCacheMetric } from './ApiCacheMetric';
import type { ApiMediaCacheMetrics } from './ApiMediaCacheMetrics';
import type { ApiRealtimeMetrics } from './ApiRealtimeMetrics';
export type ApiRuntimeMetrics = {
    camera_discovery_cache: ApiCacheMetric;
    log_sources_cache: ApiCacheMetric;
    media: ApiMediaCacheMetrics;
    peripherals_inventory_cache: ApiCacheMetric;
    pipelines_graphs_cache: ApiCacheMetric;
    pipelines_registry_cache: ApiCacheMetric;
    realtime: ApiRealtimeMetrics;
    streams_list_cache: ApiCacheMetric;
    system_metrics_cache: ApiCacheMetric;
};

