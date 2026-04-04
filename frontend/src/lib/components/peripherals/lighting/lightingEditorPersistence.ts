import { toaster } from '$lib';
import { buildErrorMessage, reportError } from '$lib/ui/errorPolicy';
import { deviceSettingsStore } from '../../../../routes/settings/deviceSettingsStore';
import type { LedConfig } from '../../../../routes/settings/types';
import { REQUESTED_BY } from '../../../../routes/settings/api';
import {
  buildAnimationImportBundle,
  buildSaveBodyFromImportedEntry,
  currentAnimationExportName,
  decodeLoadAnimationRef as decodeLightingLoadAnimationRef,
  defaultLoadAnimationRef as buildDefaultLoadAnimationRef,
  encodeLoadAnimationRef as encodeLightingLoadAnimationRef,
  hasLoadAnimationRef as hasLightingLoadAnimationRef,
  normalizeImportedEntries,
  normalizedAnimationKey,
  templateToSavedAnimation
} from './lightingAnimationLibrary';
import type {
  LightingAnimationTemplateSummary,
  SavedLightingAnimation
} from './lightingModalUtils';
import {
  deleteLightingAnimation,
  fetchLightingTemplate,
  fetchLightingTemplates,
  fetchSavedAnimations,
  resetLightingConfig as resetLightingConfigApi,
  saveLightingAnimation,
  saveLightingConfig
} from './lightingModalApi';

type LightingPersistenceState = Record<string, any>;

type LightingPersistenceDeps = {
  applyAnimationEntryToEditor: (entry: SavedLightingAnimation) => boolean;
  buildCurrentFramePayloadForOutput: () => unknown;
  buildTimelinePayloadForEditor: () => unknown;
  coerceInt: (value: number | string) => number;
  defaultLighting: LedConfig;
  isTemplateNamedAnimation: (name: string) => boolean;
  normalizeLighting: (value: unknown, fallback: LedConfig) => LedConfig;
  onRefreshRequested: () => void;
  savedPrefix: string;
  templatePrefix: string;
  validate: () => string | null;
};

export function createLightingEditorPersistence(
  state: LightingPersistenceState,
  deps: LightingPersistenceDeps
) {
  function encodeLoadAnimationRef(
    kind: 'saved' | 'template',
    value: string
  ): string {
    return encodeLightingLoadAnimationRef(
      deps.savedPrefix,
      deps.templatePrefix,
      kind,
      value
    );
  }

  function decodeLoadAnimationRef(
    value: string
  ): { kind: 'saved' | 'template'; value: string } | null {
    return decodeLightingLoadAnimationRef(
      deps.savedPrefix,
      deps.templatePrefix,
      value
    );
  }

  function hasLoadAnimationRef(ref: string): boolean {
    return hasLightingLoadAnimationRef({
      savedPrefix: deps.savedPrefix,
      templatePrefix: deps.templatePrefix,
      ref,
      visibleSavedAnimations: state.visibleSavedAnimations,
      lightingTemplates: state.lightingTemplates,
      fallbackTemplateAnimations: state.fallbackTemplateAnimations
    });
  }

  function defaultLoadAnimationRef(): string {
    return buildDefaultLoadAnimationRef({
      savedPrefix: deps.savedPrefix,
      templatePrefix: deps.templatePrefix,
      visibleSavedAnimations: state.visibleSavedAnimations,
      lightingTemplates: state.lightingTemplates,
      fallbackTemplateAnimations: state.fallbackTemplateAnimations
    });
  }

  function syncLoadAnimationRef(preferred: string = state.selectedLoadAnimationRef): void {
    state.selectedLoadAnimationRef = hasLoadAnimationRef(preferred)
      ? preferred
      : defaultLoadAnimationRef();
  }

  function selectedRefIsSavedEntry(): boolean {
    const selected = decodeLoadAnimationRef(state.selectedLoadAnimationRef);
    return Boolean(
      selected &&
        selected.kind === 'saved' &&
        !deps.isTemplateNamedAnimation(selected.value)
    );
  }

  async function loadSavedAnimations(): Promise<void> {
    state.savedBusy = true;
    const previousSelection = state.selectedLoadAnimationRef;
    try {
      state.savedAnimations = await fetchSavedAnimations();
      syncLoadAnimationRef(previousSelection);
    } catch (err) {
      state.liveError = buildErrorMessage({
        error: err,
        fallback: 'Unable to load saved animations.'
      });
      state.savedAnimations = [];
      syncLoadAnimationRef(previousSelection);
    } finally {
      state.savedBusy = false;
    }
  }

  async function loadLightingTemplates(): Promise<void> {
    state.templatesBusy = true;
    const previousSelection = state.selectedLoadAnimationRef;
    try {
      state.lightingTemplates = await fetchLightingTemplates();
      syncLoadAnimationRef(previousSelection);
    } catch (err) {
      state.animationFormError = buildErrorMessage({
        error: err,
        fallback: 'Unable to load built-in lighting templates.'
      });
      state.lightingTemplates = [];
      syncLoadAnimationRef(previousSelection);
    } finally {
      state.templatesBusy = false;
    }
  }

  async function loadSelectedAnimationIntoEditor(): Promise<void> {
    state.animationFormError = null;
    state.animationFormStatus = null;
    const selected = decodeLoadAnimationRef(state.selectedLoadAnimationRef);
    if (!selected || !selected.value.trim().length) {
      state.animationFormError = 'Select an animation or template to load.';
      return;
    }

    state.animationLoadBusy = true;
    try {
      if (selected.kind === 'saved') {
        const entry = state.visibleSavedAnimations.find(
          (item: SavedLightingAnimation) => item.name === selected.value
        );
        if (!entry) {
          state.animationFormError = 'Selected animation no longer exists.';
          return;
        }
        state.animationName = entry.name;
        if (deps.applyAnimationEntryToEditor(entry)) {
          state.animationFormStatus = `Loaded ${entry.name}`;
        } else {
          state.animationFormError =
            'This saved animation is not timeline/frame editable yet.';
        }
      } else {
        const selectedKey = normalizedAnimationKey(selected.value);
        const matchedTemplate = (
          state.lightingTemplates as LightingAnimationTemplateSummary[]
        ).find(
          (entry) =>
            entry.template_id === selected.value ||
            normalizedAnimationKey(entry.name) === selectedKey
        );
        if (matchedTemplate) {
          const template = await fetchLightingTemplate(matchedTemplate.template_id);
          const entry = templateToSavedAnimation(template);
          state.animationName = entry.name;
          if (deps.applyAnimationEntryToEditor(entry)) {
            state.animationFormStatus = `Loaded template ${template.name}`;
          } else {
            state.animationFormStatus = `Loaded template ${template.name} (effect preset, not timeline editable).`;
          }
        } else {
          const fallbackTemplate = state.fallbackTemplateAnimations.find(
            (entry: SavedLightingAnimation) =>
              normalizedAnimationKey(entry.name) === selectedKey
          );
          if (!fallbackTemplate) {
            state.animationFormError = 'Selected template no longer exists.';
            return;
          }
          state.animationName = fallbackTemplate.name;
          if (deps.applyAnimationEntryToEditor(fallbackTemplate)) {
            state.animationFormStatus = `Loaded template ${fallbackTemplate.name}`;
          } else {
            state.animationFormStatus = `Loaded template ${fallbackTemplate.name} (effect preset, not timeline editable).`;
          }
        }
      }
    } catch (err) {
      state.animationFormError = buildErrorMessage({
        error: err,
        fallback: 'Unable to load the selected item.'
      });
    } finally {
      state.animationLoadBusy = false;
    }
  }

  async function importAnimationEntries(
    entries: SavedLightingAnimation[],
    sourceLabel: string
  ): Promise<void> {
    state.animationFormError = null;
    state.animationFormStatus = null;
    if (!entries.length) {
      state.animationFormError = 'No animations found in uploaded JSON.';
      return;
    }

    state.animationImportBusy = true;
    try {
      let imported = 0;
      for (let idx = 0; idx < entries.length; idx += 1) {
        const fallbackName = `Imported Animation ${idx + 1}`;
        const body = buildSaveBodyFromImportedEntry(entries[idx], fallbackName);
        if (!body) continue;
        await saveLightingAnimation(body);
        imported += 1;
      }
      if (imported === 0) {
        state.animationFormError =
          'Imported file did not contain usable animation entries.';
        return;
      }
      await loadSavedAnimations();
      state.animationFormStatus = `Imported ${imported} animation${imported === 1 ? '' : 's'} from ${sourceLabel}`;
    } catch (err) {
      state.animationFormError = buildErrorMessage({
        error: err,
        fallback: 'Unable to import animation package.'
      });
    } finally {
      state.animationImportBusy = false;
    }
  }

  function exportAnimationsPackage(): void {
    state.animationFormError = null;
    state.animationFormStatus = null;
    const bundle = buildAnimationImportBundle(state.savedAnimations);
    const fileName = currentAnimationExportName(state.animationName);
    const blob = new Blob([JSON.stringify(bundle, null, 2)], {
      type: 'application/json'
    });
    const href = URL.createObjectURL(blob);
    const link = document.createElement('a');
    link.href = href;
    link.download = fileName;
    document.body.appendChild(link);
    link.click();
    link.remove();
    URL.revokeObjectURL(href);
    state.animationFormStatus = `Exported ${fileName}`;
  }

  function triggerAnimationUploadPicker(): void {
    state.animationUploadInput?.click();
  }

  async function handleAnimationUploadInput(event: Event): Promise<void> {
    const target = event.target as HTMLInputElement;
    const file = target.files?.[0] ?? null;
    target.value = '';
    if (!file) return;
    try {
      const text = await file.text();
      const parsed = JSON.parse(text) as unknown;
      const entries = normalizeImportedEntries(parsed);
      await importAnimationEntries(entries, file.name);
    } catch (err) {
      state.animationFormError = buildErrorMessage({
        error: err,
        fallback: 'Unable to parse uploaded animation JSON.'
      });
    }
  }

  async function saveCurrentAnimation(): Promise<void> {
    state.animationFormError = null;
    state.animationFormStatus = null;
    const name = state.animationName.trim();
    if (!name.length) {
      state.animationFormError = 'Animation name is required.';
      return;
    }

    const body: Record<string, unknown> = {
      name,
      requested_by: REQUESTED_BY,
      brightness: Math.min(255, Math.max(0, Number(state.liveBrightness) || 0))
    };

    if (state.timelineKeyframes.length > 0) {
      body.timeline = deps.buildTimelinePayloadForEditor();
      body.duration_ms = Math.max(
        0,
        Math.trunc(Number(state.timelineDurationMs) || 0)
      );
    } else {
      body.frame = deps.buildCurrentFramePayloadForOutput();
    }

    state.animationSaveBusy = true;
    try {
      await saveLightingAnimation(body);
      state.animationFormStatus = `Saved ${name}`;
      await loadSavedAnimations();
      state.selectedLoadAnimationRef = encodeLoadAnimationRef('saved', name);
      syncLoadAnimationRef(state.selectedLoadAnimationRef);
    } catch (err) {
      state.animationFormError = buildErrorMessage({
        error: err,
        fallback: 'Unable to save animation.'
      });
    } finally {
      state.animationSaveBusy = false;
    }
  }

  async function ensureDefaultAnimationEntries(
    defaultAnimations: Record<string, string>
  ): Promise<void> {
    const requiredKeys = new Set<string>();
    for (const animationName of Object.values(defaultAnimations)) {
      const key = normalizedAnimationKey(animationName);
      if (key.length > 0) {
        requiredKeys.add(key);
      }
    }
    if (requiredKeys.size === 0) {
      return;
    }

    const savedKeys = new Set<string>();
    for (const entry of state.savedAnimations as SavedLightingAnimation[]) {
      const key = normalizedAnimationKey(entry.name);
      if (key.length > 0) {
        savedKeys.add(key);
      }
    }

    const templateByNameKey = new Map<string, LightingAnimationTemplateSummary>();
    for (const template of state.lightingTemplates as LightingAnimationTemplateSummary[]) {
      const key = normalizedAnimationKey(template.name);
      if (key.length > 0 && !templateByNameKey.has(key)) {
        templateByNameKey.set(key, template);
      }
    }

    let importedTemplate = false;
    for (const key of requiredKeys) {
      if (savedKeys.has(key)) continue;
      const template = templateByNameKey.get(key);
      if (!template) continue;
      const templateDoc = await fetchLightingTemplate(template.template_id);
      const saveBody = buildSaveBodyFromImportedEntry(
        templateToSavedAnimation(templateDoc),
        templateDoc.name
      );
      if (!saveBody) continue;
      await saveLightingAnimation(saveBody);
      savedKeys.add(key);
      importedTemplate = true;
    }

    if (importedTemplate) {
      await loadSavedAnimations();
    }
  }

  async function saveLighting(): Promise<void> {
    state.status = null;
    state.error = null;
    const validation = deps.validate();
    if (validation) {
      state.error = validation;
      return;
    }

    state.busy = true;
    const brightnessValue =
      typeof state.form.brightness === 'number' ? state.form.brightness : null;
    const defaultAnimations = Object.fromEntries(
      Object.entries(state.form.default_animations ?? {})
        .map(([eventKey, animationName]) => [
          eventKey.trim().toLowerCase(),
          String(animationName ?? '').trim()
        ])
        .filter(
          ([eventKey, animationName]) =>
            eventKey.length > 0 && animationName.length > 0
        )
    );

    const payload = {
      requested_by: REQUESTED_BY,
      lighting: {
        enabled: state.form.enabled,
        gpio: deps.coerceInt(state.form.gpio),
        count: deps.coerceInt(state.form.count),
        use_pwm: state.form.use_pwm,
        color_order: state.form.color_order.trim().toLowerCase(),
        frequency_hz: deps.coerceInt(state.form.frequency_hz),
        brightness:
          brightnessValue == null
            ? null
            : Math.min(255, Math.max(0, deps.coerceInt(brightnessValue))),
        label: state.form.label?.trim() ? state.form.label.trim() : null,
        protocol: state.form.protocol.trim() || deps.defaultLighting.protocol,
        default_animations: defaultAnimations
      }
    };

    try {
      await ensureDefaultAnimationEntries(defaultAnimations);
      await saveLightingConfig(deviceSettingsStore, payload);
      state.status = `Saved at ${new Date().toLocaleTimeString([], {
        hour: '2-digit',
        minute: '2-digit'
      })}`;
      deps.onRefreshRequested();
      toaster.success({
        title: 'Lighting saved',
        description: 'LED configuration updated.'
      });
    } catch (err) {
      reportError({
        title: 'Save failed',
        error: err,
        fallback: 'Unable to save lighting settings.',
        inline: (message) => {
          state.error = message;
        }
      });
    } finally {
      state.busy = false;
    }
  }

  async function deleteSavedAnimation(
    name: string,
    source: 'panel' | 'toolbar' = 'panel'
  ): Promise<void> {
    state.savedBusy = true;
    try {
      await deleteLightingAnimation(name);
      await loadSavedAnimations();
      if (source === 'toolbar') {
        state.animationFormStatus = `Deleted ${name}`;
        if (
          state.animationName.trim().toLowerCase() === name.trim().toLowerCase()
        ) {
          state.animationName = '';
        }
      }
    } catch (err) {
      const message = buildErrorMessage({
        error: err,
        fallback: 'Unable to delete animation.'
      });
      if (source === 'toolbar') {
        state.animationFormError = message;
      } else {
        state.liveError = message;
      }
    } finally {
      state.savedBusy = false;
    }
  }

  async function deleteSelectedAnimation(): Promise<void> {
    state.animationFormError = null;
    state.animationFormStatus = null;
    const selected = decodeLoadAnimationRef(state.selectedLoadAnimationRef);
    if (
      !selected ||
      selected.kind !== 'saved' ||
      deps.isTemplateNamedAnimation(selected.value)
    ) {
      state.animationFormError = 'Select a saved animation to delete.';
      return;
    }
    const name = selected.value.trim();
    if (!name.length) {
      state.animationFormError = 'Select a saved animation to delete.';
      return;
    }
    await deleteSavedAnimation(name, 'toolbar');
  }

  async function resetLightingConfig(): Promise<void> {
    state.settingsBusy = true;
    state.settingsError = null;
    try {
      const payload = await resetLightingConfigApi();
      state.form = deps.normalizeLighting(payload, deps.defaultLighting);
      state.lightingCount = state.form.count || deps.defaultLighting.count;
      state.frequencyKhzTouched = false;
      await deviceSettingsStore.load({ force: true });
    } catch (err) {
      state.settingsError = buildErrorMessage({
        error: err,
        fallback: 'Unable to reset lighting settings.'
      });
    } finally {
      state.settingsBusy = false;
    }
  }

  function defaultAnimationRefForEvent(eventKey: string): string {
    const normalizedEvent = eventKey.trim().toLowerCase();
    if (!normalizedEvent.length) return '';
    const mapping = state.form.default_animations ?? {};
    for (const [key, value] of Object.entries(mapping)) {
      if (
        key.trim().toLowerCase() === normalizedEvent &&
        typeof value === 'string'
      ) {
        return value.trim();
      }
    }
    return '';
  }

  function setDefaultAnimationRefForEvent(
    eventKey: string,
    animationName: string
  ): void {
    const normalizedEvent = eventKey.trim().toLowerCase();
    if (!normalizedEvent.length) return;
    const next = { ...(state.form.default_animations ?? {}) } as Record<
      string,
      string
    >;
    const trimmedName = animationName.trim();
    if (!trimmedName.length) {
      delete next[normalizedEvent];
    } else {
      next[normalizedEvent] = trimmedName;
    }
    state.form = {
      ...state.form,
      default_animations: next
    };
  }

  return {
    defaultLoadAnimationRef,
    defaultAnimationRefForEvent,
    decodeLoadAnimationRef,
    deleteSelectedAnimation,
    encodeLoadAnimationRef,
    exportAnimationsPackage,
    hasLoadAnimationRef,
    handleAnimationUploadInput,
    loadLightingTemplates,
    loadSavedAnimations,
    loadSelectedAnimationIntoEditor,
    resetLightingConfig,
    saveCurrentAnimation,
    saveLighting,
    selectedRefIsSavedEntry,
    setDefaultAnimationRefForEvent,
    syncLoadAnimationRef,
    triggerAnimationUploadPicker
  };
}
