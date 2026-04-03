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
  excludedTagIdsInput: () => string;
  setExcludedTagIdsError: (message: string | null) => void;
  parseLengthToMeters: (value: string, unit?: string) => LengthValue | null;
  localizationProfiles: LocalizationProfilesStore;
};

export const createLocalizationProfileActions = (deps: LocalizationProfileActionsDeps) => {
  const parseExcludedTagIds = (raw: string): number[] | null => {
    const trimmed = raw.trim();
    if (!trimmed) return [];
    const tokens = trimmed
      .split(/[,\s]+/)
      .map((token) => token.trim())
      .filter((token) => token.length > 0);
    const parsed: number[] = [];
    for (const token of tokens) {
      if (!/^\d+$/.test(token)) {
        return null;
      }
      const value = Number(token);
      if (!Number.isSafeInteger(value) || value < 0 || value > 0xffffffff) {
        return null;
      }
      parsed.push(value);
    }
    return Array.from(new Set(parsed)).sort((left, right) => left - right);
  };

  const sameTagIdList = (left: number[] | null | undefined, right: number[] | null | undefined): boolean => {
    const a = Array.isArray(left) ? left : [];
    const b = Array.isArray(right) ? right : [];
    if (a.length !== b.length) return false;
    for (let index = 0; index < a.length; index += 1) {
      if (a[index] !== b[index]) return false;
    }
    return true;
  };

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
    // `viewEnabled` is canonical UI state and must stay explicit.
    if (profile.viewEnabled === enabled) return;
    void deps.localizationProfiles.persistProfileUpdate({ ...profile, viewEnabled: enabled });
  };

  const setProfileEnabled = (profileId: string, enabled: boolean): void => {
    const profile = deps.profiles().find((entry) => entry.id === profileId) ?? null;
    if (!profile) return;
    if ((profile.enabled ?? true) === enabled) return;
    void deps.localizationProfiles.persistProfileUpdate({ ...profile, enabled });
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
      deps.setTagSizeError(null);
      if (profile.tagSizeM == null) return;
      void deps.localizationProfiles.persistProfileUpdate({ ...profile, tagSizeM: null });
      return;
    }
    const parsed = deps.parseLengthToMeters(raw, 'm');
    const meters = parsed?.meters ?? NaN;
    if (!Number.isFinite(meters)) {
      deps.setTagSizeError('Unable to parse length (e.g. 0.03175m or 1.25in).');
      return;
    }
    deps.setTagSizeError(null);
    if (profile.tagSizeM === meters) return;
    void deps.localizationProfiles.persistProfileUpdate({ ...profile, tagSizeM: meters });
  };

  const commitExcludedTagIds = (): void => {
    const profile = deps.activeProfile();
    if (!profile) return;
    const parsed = parseExcludedTagIds(deps.excludedTagIdsInput());
    if (!parsed) {
      deps.setExcludedTagIdsError('Use comma/space-separated non-negative integer tag IDs (e.g. `1, 2 3`).');
      return;
    }
    deps.setExcludedTagIdsError(null);
    if (sameTagIdList(profile.excludedTagIds, parsed)) return;
    void deps.localizationProfiles.persistProfileUpdate({ ...profile, excludedTagIds: parsed });
  };

  return {
    persistLocalizationConfig,
    setProfileColor,
    setProfileEnabled,
    setProfileViewEnabled,
    persistProfileUpdate,
    setActiveProfile,
    addProfile,
    removeActiveProfile,
    commitProfileName,
    commitTagSize,
    commitExcludedTagIds
  };
};
