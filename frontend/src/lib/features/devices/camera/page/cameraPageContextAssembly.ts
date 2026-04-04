import { createMergedView } from '$lib/utils/liveView';

export function buildCameraPageServices<T extends object>(services: T): T {
  return services;
}

export function buildCameraPageHelpers<T extends object>(helpers: T): T {
  return helpers;
}

export function mergeCameraPageContext<T extends object>(...sources: object[]): T {
  return createMergedView<T>(...sources);
}
