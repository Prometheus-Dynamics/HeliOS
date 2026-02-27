import type { LocalizationConfig, LocalizationProfile } from '$lib/features/localization/localizationConfig';
import type { LengthValue } from '$lib/utils/units';

export type LocalizationProfilesStore = {
  persist: (config: LocalizationConfig) => Promise<LocalizationConfig | null>;
  persistProfileUpdate: (profile: LocalizationProfile) => Promise<void>;
  setActiveProfile: (profileId: string) => Promise<void>;
  addProfile: () => void;
  removeActiveProfile: () => void;
};

export type LocalizationProfileActionsDeps = {
  profiles: () => LocalizationProfile[];
  activeProfile: () => LocalizationProfile | null;
  profileNameInput: () => string;
  tagSizeInput: () => string;
  setTagSizeError: (message: string | null) => void;
  parseLengthToMeters: (value: string, unit?: string) => LengthValue | null;
  localizationProfiles: LocalizationProfilesStore;
};

export const createLocalizationProfileActions = (deps: LocalizationProfileActionsDeps) => {
  const persistLocalizationConfig = async (next: LocalizationConfig): Promise<void> => {
    await deps.localizationProfiles.persist(next);
  };

  const setProfileColor = (profileId: string, color: string): void => {
    const profile = deps.profiles().find((entry) => entry.id === profileId) ?? null;
    if (!profile) return;
    if (profile.color === color) return;
    void deps.localizationProfiles.persistProfileUpdate({ ...profile, color });
  };

  const setProfileViewEnabled = (profileId: string, enabled: boolean): void => {
    const profile = deps.profiles().find((entry) => entry.id === profileId) ?? null;
    if (!profile) return;
    // `viewEnabled` must be persisted as an explicit boolean.
    // Using `Boolean(...)` here breaks the "disable" path when `viewEnabled` is undefined,
    // because `Boolean(undefined) === false` would early-return and never persist `false`.
    if (profile.viewEnabled === enabled) return;
    void deps.localizationProfiles.persistProfileUpdate({ ...profile, viewEnabled: enabled });
  };

  const persistProfileUpdate = async (nextProfile: LocalizationProfile): Promise<void> => {
    await deps.localizationProfiles.persistProfileUpdate(nextProfile);
  };

  const setActiveProfile = async (nextId: string): Promise<void> => {
    await deps.localizationProfiles.setActiveProfile(nextId);
  };

  const addProfile = (): void => {
    deps.localizationProfiles.addProfile();
  };

  const removeActiveProfile = (): void => {
    deps.localizationProfiles.removeActiveProfile();
  };

  const commitProfileName = (): void => {
    const profile = deps.activeProfile();
    if (!profile) return;
    const nextName = deps.profileNameInput().trim();
    if (!nextName || nextName === profile.name) return;
    void deps.localizationProfiles.persistProfileUpdate({ ...profile, name: nextName });
  };

  const commitTagSize = (): void => {
    const profile = deps.activeProfile();
    if (!profile) return;
    const raw = deps.tagSizeInput().trim();
    if (!raw) {
      deps.setTagSizeError('Tag size is required.');
      return;
    }
    const parsed = deps.parseLengthToMeters(raw, 'm');
    const meters = parsed?.meters ?? NaN;
    if (!Number.isFinite(meters) || meters <= 0) {
      deps.setTagSizeError('Tag size must be a positive length (e.g. 0.03175m or 1.25in).');
      return;
    }
    deps.setTagSizeError(null);
    if (profile.tagSizeM === meters) return;
    void deps.localizationProfiles.persistProfileUpdate({ ...profile, tagSizeM: meters });
  };

  return {
    persistLocalizationConfig,
    setProfileColor,
    setProfileViewEnabled,
    persistProfileUpdate,
    setActiveProfile,
    addProfile,
    removeActiveProfile,
    commitProfileName,
    commitTagSize
  };
};
