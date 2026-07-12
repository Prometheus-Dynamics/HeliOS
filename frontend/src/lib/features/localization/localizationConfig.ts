import { createDomainResource } from '$lib/api/domainResources';

export type LocalizationSource = {
  enabled: boolean;
  streamId?: string | null;
  cameraUid?: string | null;
};

export type LocalizationProfile = {
  id: string;
  name?: string | null;
  sources?: LocalizationSource[] | null;
};

export type LocalizationConfig = {
  profiles: LocalizationProfile[];
};

const EMPTY_LOCALIZATION_CONFIG: LocalizationConfig = {
  profiles: []
};

async function fetchLocalizationConfig(): Promise<LocalizationConfig> {
  return EMPTY_LOCALIZATION_CONFIG;
}

export const localizationConfigResource = createDomainResource({
  key: 'localization:config:v1',
  loader: fetchLocalizationConfig,
  staleMs: 30_000,
  maxAgeMs: 120_000,
  kinds: ['localization', 'settings']
});
