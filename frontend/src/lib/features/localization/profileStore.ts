import { derived, get, writable } from 'svelte/store';
import {
  DEFAULT_FIELD_ORIGIN,
  DEFAULT_TEMPORAL_STABILIZATION,
  fetchLocalizationConfig,
  updateLocalizationConfig,
  type LocalizationConfig,
  type LocalizationProfile
} from './localizationConfig';

export type LocalizationProfileStore = ReturnType<typeof createLocalizationProfileStore>;

export function createLocalizationProfileStore() {
  const config = writable<LocalizationConfig | null>(null);
  const loading = writable(false);
  const error = writable<string | null>(null);
  const saving = writable(false);
  const activeProfileId = writable<string | null>(null);
  let persistChain: Promise<unknown> = Promise.resolve();
  let persistInFlight = 0;

  const profiles = derived(config, ($config) => $config?.profiles ?? []);
  const activeProfile = derived([config, activeProfileId], ([$config, $activeId]) => {
    if (!$config) return null;
    const id = $activeId ?? $config.activeProfileId ?? $config.profiles[0]?.id ?? null;
    if (!id) return null;
    return $config.profiles.find((profile) => profile.id === id) ?? null;
  });

  async function load(): Promise<void> {
    loading.set(true);
    error.set(null);
    try {
      const next = await fetchLocalizationConfig();
      config.set(next);
      error.set(null);
      const nextActiveId = next.activeProfileId ?? next.profiles[0]?.id ?? null;
      activeProfileId.set(nextActiveId);
    } catch (err) {
      const message = err instanceof Error ? err.message : 'Failed to load localization config';
      error.set(message);
      throw err;
    } finally {
      loading.set(false);
    }
  }

  async function persist(next: LocalizationConfig): Promise<LocalizationConfig | null> {
    // Serialize persists to prevent out-of-order HTTP responses from reintroducing stale state
    // (e.g. toggling sources quickly causing an older response to "re-enable" a source).
    persistInFlight += 1;
    saving.set(true);

    // Optimistic local update so the UI updates immediately and subsequent persists compose on top.
    config.set(next);
    error.set(null);
    const optimisticActiveId = next.activeProfileId ?? next.profiles[0]?.id ?? get(activeProfileId) ?? null;
    activeProfileId.set(optimisticActiveId);

    const run = async (): Promise<LocalizationConfig> => {
      const updated = await updateLocalizationConfig(next);
      config.set(updated);
      error.set(null);
      const nextActiveId = updated.activeProfileId ?? updated.profiles[0]?.id ?? get(activeProfileId) ?? null;
      activeProfileId.set(nextActiveId);
      return updated;
    };

    // Keep the chain alive even if an earlier persist fails.
    const op = persistChain.then(run, run);
    persistChain = op.then(
      () => null,
      () => null
    );

    try {
      return await op;
    } catch (err) {
      const message = err instanceof Error ? err.message : 'Failed to update localization config';
      error.set(message);
      throw err;
    } finally {
      persistInFlight -= 1;
      if (persistInFlight <= 0) {
        persistInFlight = 0;
        saving.set(false);
      }
    }
  }

  async function persistProfileUpdate(nextProfile: LocalizationProfile): Promise<void> {
    const current = get(config);
    if (!current) return;
    const nextActiveId = current.activeProfileId ?? get(activeProfileId) ?? nextProfile.id;
    const next = {
      ...current,
      activeProfileId: nextActiveId,
      profiles: current.profiles.map((profile) => (profile.id === nextProfile.id ? nextProfile : profile))
    };
    await persist(next);
  }

  async function setActiveProfile(nextId: string): Promise<void> {
    const current = get(config);
    if (!current) return;
    // Active profile selection should not implicitly change "Views" visibility toggles.
    const next = { ...current, activeProfileId: nextId };
    await persist(next);
  }

  function nextProfileName(existing: LocalizationProfile[]): string {
    const base = existing.length + 1;
    return `Profile ${base}`;
  }

  function createProfileId(): string {
    if (typeof crypto !== 'undefined' && 'randomUUID' in crypto) {
      return crypto.randomUUID();
    }
    return `profile-${Date.now()}`;
  }

  function addProfile(): void {
    const current = get(config);
    if (!current) return;
    const nextProfile: LocalizationProfile = {
      id: createProfileId(),
      name: nextProfileName(current.profiles),
      tagSizeM: null,
      allowedTagIds: [],
      excludedTagIds: [],
      fieldMapId: null,
      fieldOrigin: { ...DEFAULT_FIELD_ORIGIN },
      snapZToGround: false,
      snapRollToGround: false,
      snapPitchToGround: false,
      enabled: true,
      color: null,
      viewEnabled: true,
      temporalStabilization: { ...DEFAULT_TEMPORAL_STABILIZATION },
      sources: [],
      solvers: []
    };
    const next = {
      ...current,
      activeProfileId: nextProfile.id,
      profiles: [...current.profiles, nextProfile]
    };
    activeProfileId.set(nextProfile.id);
    config.set(next);
    void persist(next);
  }

  function removeActiveProfile(): void {
    const current = get(config);
    const currentActive = get(activeProfile);
    if (!current || !currentActive) return;
    if (current.profiles.length <= 1) return;
    const nextProfiles = current.profiles.filter((profile) => profile.id !== currentActive.id);
    const nextActiveId = nextProfiles[0]?.id ?? null;
    const next = { ...current, activeProfileId: nextActiveId, profiles: nextProfiles };
    activeProfileId.set(nextActiveId);
    config.set(next);
    void persist(next);
  }

  return {
    config,
    profiles,
    activeProfile,
    activeProfileId,
    loading,
    error,
    saving,
    load,
    persist,
    persistProfileUpdate,
    setActiveProfile,
    addProfile,
    removeActiveProfile
  };
}
