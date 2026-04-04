export function buildCameraPageServices<T extends object>(services: T): T {
  return services;
}

export function buildCameraPageHelpers<T extends object>(helpers: T): T {
  return helpers;
}

export function mergeCameraPageContext<T>(...sources: object[]): T {
  const merged: Record<string, unknown> = {};
  for (const source of sources) {
    Object.defineProperties(merged, Object.getOwnPropertyDescriptors(source));
  }
  return merged as T;
}
