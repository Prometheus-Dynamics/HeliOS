<script lang="ts">
  import { onDestroy, onMount } from 'svelte';
  import { subscribeDomainInvalidations } from '$lib/api/invalidation';
  import { scheduleAfterPaint, scheduleWhenIdle } from '$lib/utils/browserSchedule';
  import { createMergedView } from '$lib/utils/liveView';
  import LocalizationPageRouteContent from './LocalizationPageRouteContent.svelte';
  import type { LocalizationPageRouteState } from './localizationPageRouteTypes';
  import { createLocalizationPageRouteCore } from './localizationPageRouteCore.svelte';
  import { createLocalizationPageRouteProfileState } from './localizationPageRouteProfileState.svelte';
  import { createLocalizationPageRouteViewerState } from './localizationPageRouteViewerState.svelte';
  import {
    LIVE_SOURCES_REFRESH_MIN_INTERVAL_MS,
    LIVE_UPDATES_REFRESH_DEBOUNCE_MS,
    shouldApplyLiveUpdate,
    shouldRefreshSourcesForLiveUpdate
  } from './localizationPageRouteSupport';

  const core = createLocalizationPageRouteCore();
  const profile = createLocalizationPageRouteProfileState(core);
  const viewer = createLocalizationPageRouteViewerState(core, profile);
  core.setDerivedStateBindings({
    getActiveSolverConfig: () => profile.activeSolverConfig,
    getPrimaryCameraKey: () => profile.primaryCameraKey,
    getOriginFromFieldCenterForEditor: () => viewer.originFromFieldCenterForEditor,
    getSelectedCustomField: () => viewer.selectedCustomField,
    getMaxMapUploadBytes: () => profile.maxMapUploadBytes
  });

  const derivedState = {
    get activeProfile() {
      return core.activeProfile.current;
    },
    get activeProfileId() {
      return core.activeProfileId.current;
    },
    get localizationConfig() {
      return core.localizationConfig.current;
    },
    get localizationConfigLoading() {
      return core.localizationConfigLoading.current;
    },
    get profiles() {
      return core.profiles.current;
    },
    get hasLocalizationBootstrapData() {
      return Boolean((core.localizationConfig.current?.profiles?.length ?? 0) || state.sources.length || state.fieldMaps.length);
    },
    get showLocalizationBootLoading() {
      return state.localizationBootLoading && !state.hasLocalizationBootstrapData;
    },
    get isSourceCalibrated() {
      return core.isSourceCalibrated;
    },
    get poseSpaceLabel() {
      return core.poseSpaceLabel;
    }
  };

  const handleProfileImportInput = (event: Event): void => {
    const input = event.currentTarget;
    if (!(input instanceof HTMLInputElement)) return;
    const file = input.files?.[0] ?? null;
    if (!file) return;
    void core.importLocalizationProfiles(file);
  };

  const handleProfileColorInput = (profileId: string, event: Event): void => {
    const input = event.currentTarget;
    if (!(input instanceof HTMLInputElement)) return;
    core.setProfileColor(profileId, input.value);
  };

  const refreshLocalizationLiveState = (options: { refreshSources?: boolean } = {}): void => {
    void core.rigLayoutStore.refresh({ force: true });
    if (!options.refreshSources) return;
    const now = Date.now();
    if (now - state.lastLiveSourcesRefreshAtMs >= LIVE_SOURCES_REFRESH_MIN_INTERVAL_MS) {
      state.lastLiveSourcesRefreshAtMs = now;
      void core.loadSources();
    }
    void core.loadLocalizationConfig();
    void core.loadStreamsSnapshot();
    void core.loadFieldMapList();
  };

  const scheduleLiveUpdatesRefresh = (event?: Parameters<typeof shouldApplyLiveUpdate>[0]): void => {
    if (!core.browser) return;
    if (event && shouldRefreshSourcesForLiveUpdate(event)) {
      state.liveUpdatesRefreshSourcesPending = true;
    }
    if (state.liveUpdatesRefreshHandle != null) return;
    state.liveUpdatesRefreshHandle = window.setTimeout(() => {
      state.liveUpdatesRefreshHandle = null;
      if (document.hidden) return;
      const refreshSources = state.liveUpdatesRefreshSourcesPending;
      state.liveUpdatesRefreshSourcesPending = false;
      refreshLocalizationLiveState({ refreshSources });
    }, LIVE_UPDATES_REFRESH_DEBOUNCE_MS);
  };

  const bootstrapLocalizationPage = async (): Promise<void> => {
    state.localizationBootLoading = true;
    state.localizationBootError = null;
    try {
      await Promise.all([core.rigLayoutStore.refresh(), core.loadLocalizationCapabilities(), core.loadLocalizationConfig()]);
      await Promise.all([core.loadSources(), core.loadStreamsSnapshot(), core.loadFieldMapList()]);
      const seeded = await core.maybeSeedDefaultLocalizationProfile();
      if (seeded) {
        await Promise.all([core.loadLocalizationConfig(), core.loadSources(), core.loadStreamsSnapshot(), core.loadFieldMapList()]);
      }
    } catch (error) {
      state.localizationBootError =
        error instanceof Error ? error.message : 'Failed to initialize localization page';
    } finally {
      if (!state.localizationDisposed) {
        state.localizationBootLoading = false;
      }
    }
  };

  const retryLocalizationBootstrap = (): void => {
    state.cancelLocalizationBootstrap?.();
    state.cancelLocalizationBootstrap = scheduleAfterPaint(() => {
      if (!state.localizationDisposed) {
        void bootstrapLocalizationPage();
      }
    }, 1);
  };

  const routeHandlers = {
    handleProfileColorInput,
    handleProfileImportInput,
    retryLocalizationBootstrap
  };

  const state = createMergedView<LocalizationPageRouteState>(
    core.state,
    core,
    profile,
    viewer,
    derivedState,
    routeHandlers
  );

  $effect(() => {
    void profile.hasAnyFeedSources;
    if (!profile.hasAnyFeedSources) {
      profile.feedPoller.stop();
      state.feedStatus = 'idle';
      state.feedMessage = null;
      return;
    }
    if (!profile.feedPoller.isBusy()) {
      state.feedStatus = 'connecting';
      state.feedMessage = null;
      profile.feedPoller.schedule(0);
    }
  });

  $effect(() => {
    if (!state.showCustomFieldsOverlay) return;
    state.mapAssignId = viewer.selectedFieldMapId ?? '';
  });

  onMount(() => {
    state.localizationDisposed = false;
    state.stopLiveUpdates = subscribeDomainInvalidations(
      ['localization', 'streams', 'pipelines', 'media', 'imu', 'device', 'settings'],
      (event) => {
        if (!shouldApplyLiveUpdate(event)) return;
        scheduleLiveUpdatesRefresh(event);
      }
    );
    retryLocalizationBootstrap();
    state.cancelLocalizationViewersWarmup = scheduleWhenIdle(() => {
      if (!state.localizationDisposed) {
        void core.loadLocalizationViewers();
      }
    }, { timeoutMs: 2200, fallbackMs: 900 });
    core.localizationStorage.loadFromStorage();
    state.selectedCustomFieldId = core.customFields.current[0]?.id ?? null;
    state.selectedCustomFieldOriginId = core.customFields.current[0]?.origins[0]?.id ?? null;
    if (core.browser) {
      state.visibilityHandler = () => {
        if (document.hidden) {
          state.pollVisibilityPaused = true;
          profile.feedPoller.stop();
        } else if (state.pollVisibilityPaused) {
          state.pollVisibilityPaused = false;
          if (profile.hasAnyFeedSources) {
            profile.feedPoller.schedule(0);
          }
        }
      };
      document.addEventListener('visibilitychange', state.visibilityHandler);
    }
  });

  onDestroy(() => {
    state.localizationDisposed = true;
    state.cancelLocalizationBootstrap?.();
    state.cancelLocalizationBootstrap = null;
    state.cancelLocalizationViewersWarmup?.();
    state.cancelLocalizationViewersWarmup = null;
    state.stopLiveUpdates?.();
    state.stopLiveUpdates = null;
    if (state.liveUpdatesRefreshHandle != null) {
      clearTimeout(state.liveUpdatesRefreshHandle);
      state.liveUpdatesRefreshHandle = null;
    }
    state.liveUpdatesRefreshSourcesPending = false;
    core.rigLayoutUnsubscribe();
    profile.feedPoller.stop();
    for (const cleanup of core.streamMetricsCleanup.values()) {
      cleanup();
    }
    core.streamMetricsCleanup.clear();
    if (state.visibilityHandler) {
      document.removeEventListener('visibilitychange', state.visibilityHandler);
      state.visibilityHandler = null;
    }
  });
</script>

<LocalizationPageRouteContent {state} />
