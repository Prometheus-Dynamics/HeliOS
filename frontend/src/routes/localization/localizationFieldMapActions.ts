import type { CustomField, CustomFieldOrigin } from '$lib/features/localization/types';
import type { FieldMapDocument, FieldMapSummary } from '$lib/features/localization/fieldMaps';
import type { LocalizationProfile } from '$lib/features/localization/localizationConfig';
import type { LengthValue } from '$lib/utils/units';

export type LocalizationFieldMapDeps = {
  getCustomFields: () => CustomField[];
  setCustomFields: (next: CustomField[]) => void;
  getSelectedCustomFieldId: () => string | null;
  setSelectedCustomFieldId: (next: string | null) => void;
  setSelectedCustomFieldOriginId: (next: string | null) => void;
  getSelectedCustomField: () => CustomField | null;
  setNewCustomFieldError: (message: string | null) => void;
  getNewCustomFieldName: () => string;
  getNewCustomFieldWidth: () => string;
  getNewCustomFieldDepth: () => string;
  parseLengthToMeters: (value: string, unit?: string) => LengthValue | null;
  setViewMode: (mode: 'isolated' | 'frc-field' | 'custom-field') => void;
  getActiveProfile: () => LocalizationProfile | null;
  persistProfileUpdate: (profile: LocalizationProfile) => Promise<void> | void;
  updateProfileFieldMapId: (profile: LocalizationProfile, mapId: string | null) => LocalizationProfile;
  fetchFieldMap: (id: string) => Promise<FieldMapDocument>;
  listFieldMaps: () => Promise<FieldMapSummary[]>;
  uploadLimelightFmap: (file: File) => Promise<FieldMapSummary>;
  getFieldMaps: () => FieldMapSummary[];
  setFieldMaps: (next: FieldMapSummary[]) => void;
  getFieldMapDocs: () => Record<string, FieldMapDocument>;
  setFieldMapDocs: (next: Record<string, FieldMapDocument>) => void;
  getFieldMapDocErrors: () => Record<string, string>;
  setFieldMapDocErrors: (next: Record<string, string>) => void;
  setFieldMapsLoading: (loading: boolean) => void;
  setFieldMapsError: (message: string | null) => void;
  getMapUploadFile: () => File | null;
  setMapUploadFile: (file: File | null) => void;
  setMapUploadBusy: (busy: boolean) => void;
  setMapUploadError: (message: string | null) => void;
  setMapUploadSuccess: (summary: FieldMapSummary) => void;
  toaster: { success: (payload: { title: string; description?: string }) => void; error: (payload: { title: string; description?: string }) => void };
  getNewOriginName: () => string;
  getNewOriginX: () => string;
  getNewOriginZ: () => string;
  getNewOriginYaw: () => string;
  setNewOriginError: (message: string | null) => void;
  toNumber: (value: string, fallback: number) => number;
};

export const createLocalizationFieldMapActions = (deps: LocalizationFieldMapDeps) => {
  const hasValidTagSize = (value: number | null | undefined): boolean =>
    typeof value === 'number' && Number.isFinite(value) && value > 0;

  const inferTagSizeFromFieldMap = (doc: FieldMapDocument): number | null => {
    const sizes = (doc.markers ?? [])
      .map((marker) => Number(marker.sizeM))
      .filter((size) => Number.isFinite(size) && size > 0);
    if (sizes.length === 0) return null;

    // Prefer the most common tag size in mixed maps.
    const buckets = new Map<string, { size: number; count: number }>();
    for (const size of sizes) {
      const key = size.toFixed(6);
      const existing = buckets.get(key);
      if (existing) {
        existing.count += 1;
      } else {
        buckets.set(key, { size, count: 1 });
      }
    }

    let best: { size: number; count: number } | null = null;
    for (const entry of buckets.values()) {
      if (!best || entry.count > best.count) best = entry;
    }
    return best?.size ?? null;
  };

  const fieldMapDocLooksHydrated = (doc: FieldMapDocument): boolean => {
    if (!doc.markers.length) return true;
    const sample = doc.markers[0];
    const bits = sample?.tagBits;
    return Boolean(bits && Array.isArray(bits.rows) && typeof bits.width === 'number' && bits.rows.length === bits.width);
  };

  const createCustomField = (): void => {
    const width = deps.parseLengthToMeters(deps.getNewCustomFieldWidth(), 'm')?.meters ?? NaN;
    const depth = deps.parseLengthToMeters(deps.getNewCustomFieldDepth(), 'm')?.meters ?? NaN;
    if (!Number.isFinite(width) || width <= 0 || !Number.isFinite(depth) || depth <= 0) {
      deps.setNewCustomFieldError('Width/depth must be valid lengths (e.g. 16.46m, 54ft, 8.23m).');
      return;
    }

    const name = deps.getNewCustomFieldName().trim() || 'Custom field';
    const id = typeof crypto !== 'undefined' && 'randomUUID' in crypto ? crypto.randomUUID() : `${Date.now()}`;
    const next = [
      ...deps.getCustomFields(),
      { id, name, width, depth, origins: [{ id: 'center', name: 'Center', x: 0, z: 0, yawDeg: 0 }], mapId: null }
    ];
    deps.setCustomFields(next);
    deps.setSelectedCustomFieldId(id);
    deps.setSelectedCustomFieldOriginId('center');
    deps.setNewCustomFieldError(null);
  };

  const loadFieldMapList = async (): Promise<void> => {
    deps.setFieldMapsLoading(true);
    deps.setFieldMapsError(null);
    try {
      deps.setFieldMaps(await deps.listFieldMaps());
    } catch (error) {
      deps.setFieldMapsError(error instanceof Error ? error.message : 'Unable to load field maps');
    } finally {
      deps.setFieldMapsLoading(false);
    }
  };

  const ensureFieldMapLoaded = async (id: string): Promise<void> => {
    if (!id) return;
    const existing = deps.getFieldMapDocs()[id] ?? null;
    if (existing && fieldMapDocLooksHydrated(existing)) {
      return;
    }
    try {
      const doc = await deps.fetchFieldMap(id);
      deps.setFieldMapDocs({ ...deps.getFieldMapDocs(), [id]: doc });
      if (deps.getFieldMapDocErrors()[id]) {
        const nextErrors = { ...deps.getFieldMapDocErrors() };
        delete nextErrors[id];
        deps.setFieldMapDocErrors(nextErrors);
      }
    } catch (error) {
      const message = error instanceof Error ? error.message : 'Unable to load field map';
      deps.setFieldMapDocErrors({ ...deps.getFieldMapDocErrors(), [id]: message });
    }
  };

  const getOrLoadFieldMapDoc = async (id: string): Promise<FieldMapDocument | null> => {
    if (!id) return null;
    const existing = deps.getFieldMapDocs()[id] ?? null;
    if (existing && fieldMapDocLooksHydrated(existing)) {
      return existing;
    }
    try {
      const doc = await deps.fetchFieldMap(id);
      deps.setFieldMapDocs({ ...deps.getFieldMapDocs(), [id]: doc });
      if (deps.getFieldMapDocErrors()[id]) {
        const nextErrors = { ...deps.getFieldMapDocErrors() };
        delete nextErrors[id];
        deps.setFieldMapDocErrors(nextErrors);
      }
      return doc;
    } catch (error) {
      const message = error instanceof Error ? error.message : 'Unable to load field map';
      deps.setFieldMapDocErrors({ ...deps.getFieldMapDocErrors(), [id]: message });
      return null;
    }
  };

  const createCustomFieldFromMap = (summary: FieldMapSummary): void => {
    const name = summary.name.trim() || 'Field map';
    const width = summary.widthM;
    const depth = summary.depthM;
    const id = typeof crypto !== 'undefined' && 'randomUUID' in crypto ? crypto.randomUUID() : `${Date.now()}`;
    const next: CustomField[] = [
      ...deps.getCustomFields(),
      {
        id,
        name,
        width,
        depth,
        origins: [{ id: 'center', name: 'Center', x: 0, z: 0, yawDeg: 0 }],
        mapId: summary.id
      }
    ];
    deps.setCustomFields(next);
    // Keep solves in frc-field coordinates so profile fieldOrigin controls blue/red/center/custom.
    deps.setViewMode('frc-field');
    deps.setSelectedCustomFieldId(id);
    deps.setSelectedCustomFieldOriginId('center');
    assignMapToSelectedField(summary.id);
  };

  const assignMapToSelectedField = (mapId: string | null): void => {
    const current = deps.getActiveProfile();
    if (!current) return;

    void (async () => {
      let inferredTagSize: number | null = null;
      if (mapId) {
        const doc = await getOrLoadFieldMapDoc(mapId);
        inferredTagSize = doc ? inferTagSizeFromFieldMap(doc) : null;
      }

      const profile = deps.getActiveProfile();
      if (!profile || profile.id !== current.id) return;

      let nextProfile = deps.updateProfileFieldMapId(profile, mapId);
      if (!hasValidTagSize(profile.tagSizeM) && hasValidTagSize(inferredTagSize)) {
        nextProfile = { ...nextProfile, tagSizeM: inferredTagSize };
      }
      await deps.persistProfileUpdate(nextProfile);
    })();
  };

  const handleMapUploadFile = (file: File): void => {
    if (!file.name.toLowerCase().endsWith('.fmap')) {
      deps.setMapUploadError('Please select a .fmap file.');
      return;
    }
    deps.setMapUploadError(null);
    deps.setMapUploadFile(file);
    void uploadSelectedMapFile();
  };

  const uploadSelectedMapFile = async (): Promise<void> => {
    const file = deps.getMapUploadFile();
    if (!file) return;
    deps.setMapUploadBusy(true);
    deps.setMapUploadError(null);
    try {
      const summary = await deps.uploadLimelightFmap(file);
      deps.setFieldMaps([...deps.getFieldMaps(), summary].sort((a, b) => a.name.toLowerCase().localeCompare(b.name.toLowerCase())));
      deps.setMapUploadFile(null);
      createCustomFieldFromMap(summary);
      const doc = await deps.fetchFieldMap(summary.id);
      deps.setFieldMapDocs({ ...deps.getFieldMapDocs(), [summary.id]: doc });
      deps.toaster.success({ title: 'Field map uploaded', description: summary.name });
    } catch (error) {
      const message = error instanceof Error ? error.message : 'Unable to upload field map';
      deps.setMapUploadError(message);
      deps.toaster.error({ title: 'Field map upload failed', description: message });
    } finally {
      deps.setMapUploadBusy(false);
    }
  };

  const addOriginToSelectedField = (): void => {
    const field = deps.getSelectedCustomField();
    if (!field) return;
    const name = deps.getNewOriginName().trim() || 'Origin';
    const x = deps.parseLengthToMeters(deps.getNewOriginX(), 'm')?.meters ?? NaN;
    const z = deps.parseLengthToMeters(deps.getNewOriginZ(), 'm')?.meters ?? NaN;
    const yawDeg = deps.toNumber(deps.getNewOriginYaw(), NaN);
    if (![x, z, yawDeg].every((value) => Number.isFinite(value))) {
      deps.setNewOriginError('Origin x/z must be valid lengths and yaw must be degrees.');
      return;
    }
    const id = typeof crypto !== 'undefined' && 'randomUUID' in crypto ? crypto.randomUUID() : `${Date.now()}`;
    const origin: CustomFieldOrigin = { id, name, x, z, yawDeg };
    const next = deps.getCustomFields().map((entry) =>
      entry.id === field.id ? { ...entry, origins: [...entry.origins, origin] } : entry
    );
    deps.setCustomFields(next);
    deps.setSelectedCustomFieldOriginId(id);
    deps.setNewOriginError(null);
  };

  return {
    createCustomField,
    loadFieldMapList,
    ensureFieldMapLoaded,
    createCustomFieldFromMap,
    assignMapToSelectedField,
    handleMapUploadFile,
    uploadSelectedMapFile,
    addOriginToSelectedField
  };
};
