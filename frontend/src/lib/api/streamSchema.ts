export const CURRENT_STREAM_CONFIG_SCHEMA_VERSION = 1;

export function withCurrentStreamManifestSchema<T extends object>(manifest: T): T & { schema_version: number } {
  return {
    schema_version: CURRENT_STREAM_CONFIG_SCHEMA_VERSION,
    ...manifest
  };
}
