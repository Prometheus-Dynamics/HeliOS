import type { LocalizationPipelineStatus } from '$lib/features/localization/localizationPipeline';
import type { LocalizationPipelineSource } from '$lib/features/localization/pipelineSources';
import {
  fetchLocalizationPipelineOutputs,
  fetchLocalizationPipelineStatus
} from '$lib/features/localization/localizationPipeline';
import {
  fetchLocalizationPipelineSources,
  isLocalizationCompatibleSource
} from '$lib/features/localization/pipelineSources';

type FeedStatus = 'idle' | 'connecting' | 'live' | 'error';

type LocalizationProfileLoader = { load: () => Promise<void> };

export const createLocalizationPageState = (options: {
  localizationProfiles: LocalizationProfileLoader;
  setFeedStatus: (status: FeedStatus) => void;
  setFeedMessage: (message: string | null) => void;
  setPipelineStatus: (status: LocalizationPipelineStatus | null) => void;
  setPipelineStatusLoading: (loading: boolean) => void;
  setPipelineStatusError: (message: string | null) => void;
  setPipelineOutputs: (outputs: string[]) => void;
  setPipelineOutputsLoading: (loading: boolean) => void;
  setPipelineOutputsError: (message: string | null) => void;
  setSources: (sources: LocalizationPipelineSource[]) => void;
  setSourcesLoading: (loading: boolean) => void;
  setSourcesError: (message: string | null) => void;
  setSourceCompatibility: (compat: Record<string, boolean>) => void;
  getSelectedSourceIds: () => string[];
  applySourceSelection: (nextIds: string[]) => void;
  toaster: { error: (payload: { title: string; description?: string }) => void };
  isBrowser: boolean;
  getLocalizationViewersComponent: () => unknown | null;
  setLocalizationViewersComponent: (component: unknown) => void;
}) => {
  const evaluateSourceCompatibility = async (
    nextSources: LocalizationPipelineSource[]
  ): Promise<Record<string, boolean>> => {
    const compatibility: Record<string, boolean> = {};
    for (const source of nextSources) {
      compatibility[source.id] = isLocalizationCompatibleSource(source);
    }
    return compatibility;
  };

  const loadLocalizationConfig = async (): Promise<void> => {
    try {
      await options.localizationProfiles.load();
    } catch (error) {
      const message = error instanceof Error ? error.message : 'Failed to load localization config';
      options.setFeedStatus('error');
      options.setFeedMessage(message);
    }
  };

  const loadPipelineStatus = async (profileId?: string | null): Promise<void> => {
    options.setPipelineStatusLoading(true);
    options.setPipelineStatusError(null);
    try {
      options.setPipelineStatus(await fetchLocalizationPipelineStatus(profileId));
    } catch (error) {
      options.setPipelineStatus(null);
      options.setPipelineStatusError(error instanceof Error ? error.message : 'Failed to load pipeline status');
    } finally {
      options.setPipelineStatusLoading(false);
    }
  };

  const loadPipelineOutputs = async (profileId?: string | null): Promise<void> => {
    options.setPipelineOutputsLoading(true);
    options.setPipelineOutputsError(null);
    try {
      options.setPipelineOutputs(await fetchLocalizationPipelineOutputs(profileId));
    } catch (error) {
      if (error instanceof Error && error.message === 'Not found') {
        options.setPipelineOutputs([]);
      } else {
        options.setPipelineOutputsError(error instanceof Error ? error.message : 'Failed to load pipeline outputs');
      }
    } finally {
      options.setPipelineOutputsLoading(false);
    }
  };

  const loadSources = async (): Promise<void> => {
    options.setSourcesLoading(true);
    options.setSourcesError(null);
    try {
      const nextSources = await fetchLocalizationPipelineSources();
      const compatibility = await evaluateSourceCompatibility(nextSources);
      options.setSourceCompatibility(compatibility);
      options.setSources(nextSources);
    } catch (error) {
      const message = error instanceof Error ? error.message : 'Unable to load localization sources';
      options.setSourcesError(message);
      options.setFeedStatus('error');
      options.setFeedMessage(message);
      options.toaster.error({ title: 'Localization sources unavailable', description: message });
    } finally {
      options.setSourcesLoading(false);
    }
  };

  const loadLocalizationViewers = async (): Promise<void> => {
    if (!options.isBrowser || options.getLocalizationViewersComponent()) return;
    const module = await import('$lib/components/LocalizationViewers.svelte');
    options.setLocalizationViewersComponent(module.default);
  };

  return {
    loadLocalizationConfig,
    loadPipelineStatus,
    loadPipelineOutputs,
    loadSources,
    loadLocalizationViewers
  };
};
