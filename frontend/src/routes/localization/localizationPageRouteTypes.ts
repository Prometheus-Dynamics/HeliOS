import type { LocalizationPageRouteCore } from './localizationPageRouteCore.svelte';
import type { LocalizationPageRouteProfileState } from './localizationPageRouteProfileState.svelte';
import type { LocalizationPageRouteViewerState } from './localizationPageRouteViewerState.svelte';

type LocalizationPageRouteDerivedState = {
  activeProfile: LocalizationPageRouteCore['activeProfile']['current'];
  activeProfileId: LocalizationPageRouteCore['activeProfileId']['current'];
  localizationConfig: LocalizationPageRouteCore['localizationConfig']['current'];
  localizationConfigLoading: LocalizationPageRouteCore['localizationConfigLoading']['current'];
  profiles: LocalizationPageRouteCore['profiles']['current'];
  hasLocalizationBootstrapData: boolean;
  showLocalizationBootLoading: boolean;
  isSourceCalibrated: LocalizationPageRouteCore['isSourceCalibrated'];
  poseSpaceLabel: LocalizationPageRouteCore['poseSpaceLabel'];
  handleProfileColorInput: (profileId: string, event: Event) => void;
  handleProfileImportInput: (event: Event) => void;
  retryLocalizationBootstrap: () => void;
};

export type LocalizationPageRouteState =
  LocalizationPageRouteCore['state'] &
  Omit<LocalizationPageRouteCore, 'state'> &
  LocalizationPageRouteProfileState &
  LocalizationPageRouteViewerState &
  LocalizationPageRouteDerivedState;
