import type {
  LocalizationPoseSpace,
  LocalizationProfile,
  LocalizationSolverConfig,
  LocalizationSolverMode
} from '$lib/features/localization/localizationConfig';
import type { LocalizationPipelineSource } from '$lib/features/localization/pipelineSources';
import {
  buildOutputSpacesFromSolve,
  normalizeInputKey,
  updateProfilePipelineTemplate,
  updateProfileSourceInputKey,
  updateProfileSources,
  updateSolverOutputSpaces
} from '$lib/features/localization/utils';

export type FeedStatus = 'idle' | 'connecting' | 'live' | 'error';

type FeedPoller = { stop: () => void; schedule: (delayMs: number) => void };

type PersistProfileUpdate = (next: LocalizationProfile) => void | Promise<void>;

type LocalizationPageActionsOptions = {
  getActiveProfile: () => LocalizationProfile | null;
  getActiveSolverConfig: () => LocalizationSolverConfig | null;
  getSolvePoseSpaces: () => LocalizationPoseSpace[];
  getSupportedPoseSpaces?: () => LocalizationPoseSpace[];
  setFieldMapSelection: (next: string) => void;
  assignMapToSelectedField: (mapId: string | null) => void;
  persistProfileUpdate: PersistProfileUpdate;
  feedPoller: FeedPoller;
  getSelectedSourceIds: () => string[];
  setSelectedSourceIds: (next: string[]) => void;
  getPrimarySourceId: () => string | null;
  setPrimarySourceId: (next: string | null) => void;
  getSources: () => LocalizationPipelineSource[];
  setSolveResponse: (next: unknown | null) => void;
  setRawMarkers: (next: unknown[]) => void;
  setPollResults: (next: unknown[]) => void;
  setLastPollMs: (next: number | null) => void;
  hasAnyFeedSources: () => boolean;
  setFeedStatus: (status: FeedStatus) => void;
  setFeedMessage: (message: string | null) => void;
};

export const createLocalizationPageActions = (options: LocalizationPageActionsOptions) => {
  const setFieldMapSelection = (nextId: string): void => {
    const normalized = nextId.trim();
    const mapId = normalized.length > 0 ? normalized : null;
    options.setFieldMapSelection(normalized);
    options.assignMapToSelectedField(mapId);
    if (!mapId) return;

    const profile = options.getActiveProfile();
    const activeSolverConfig = options.getActiveSolverConfig();
    if (!profile || !activeSolverConfig) return;

    const requiredFieldSpaces: LocalizationPoseSpace[] = ['camera_in_field', 'robot_in_field'];
    const supportedPoseSpaces = new Set<LocalizationPoseSpace>(
      options.getSupportedPoseSpaces?.() ?? requiredFieldSpaces
    );
    const currentOutputs = activeSolverConfig.outputSpaces ?? [];
    const missing = requiredFieldSpaces.filter(
      (space) => supportedPoseSpaces.has(space) && !currentOutputs.includes(space)
    );
    if (missing.length === 0) return;

    void options.persistProfileUpdate(
      updateSolverOutputSpaces(profile, activeSolverConfig.id, [...currentOutputs, ...missing])
    );
  };

  const toggleSolverOutputSpace = (solverId: string, space: LocalizationPoseSpace, enabled: boolean): void => {
    const profile = options.getActiveProfile();
    if (!profile) return;
    const solver = profile.solvers.find((entry) => entry.id === solverId);
    if (!solver) return;
    const exists = solver.outputSpaces.includes(space);
    if (enabled && exists) return;
    if (!enabled && !exists) return;
    const nextSpaces = enabled
      ? [...solver.outputSpaces, space]
      : solver.outputSpaces.filter((entry) => entry !== space);
    void options.persistProfileUpdate(updateSolverOutputSpaces(profile, solverId, nextSpaces));
  };

  const setActiveSolverMode = (mode: LocalizationSolverMode): void => {
    const profile = options.getActiveProfile();
    const activeSolverConfig = options.getActiveSolverConfig();
    if (!profile || !activeSolverConfig) return;
    if (activeSolverConfig.mode === mode) return;
    const nextSolvers = profile.solvers.map((solver) =>
      solver.id === activeSolverConfig.id ? { ...solver, mode } : solver
    );
    void options.persistProfileUpdate({ ...profile, solvers: nextSolvers });
  };

  const setSolveSpaceEnabled = (space: LocalizationPoseSpace, enabled: boolean): void => {
    const profile = options.getActiveProfile();
    const activeSolverConfig = options.getActiveSolverConfig();
    if (!profile || !activeSolverConfig) return;
    const current = options.getSolvePoseSpaces();
    const exists = current.includes(space);
    if (enabled && exists) return;
    if (!enabled && !exists) return;
    const nextSolve = enabled ? [...current, space] : current.filter((entry) => entry !== space);
    const nextOutputs = buildOutputSpacesFromSolve(activeSolverConfig.outputSpaces ?? [], nextSolve);
    void options.persistProfileUpdate(updateSolverOutputSpaces(profile, activeSolverConfig.id, nextOutputs));
  };

  const setPipelineTemplateId = (templateId: string): void => {
    const profile = options.getActiveProfile();
    if (!profile) return;
    const normalized = templateId.trim();
    const nextId = normalized.length > 0 ? normalized : null;
    void options.persistProfileUpdate(updateProfilePipelineTemplate(profile, nextId));
  };

  const setSourceInputKey = (sourceId: string, rawValue: string): void => {
    const profile = options.getActiveProfile();
    if (!profile) return;
    const normalized = normalizeInputKey(rawValue);
    void options.persistProfileUpdate(updateProfileSourceInputKey(profile, sourceId, normalized));
  };

  const applySourceSelection = (nextIds: string[]): void => {
    options.feedPoller.stop();

    const unique = Array.from(new Set(nextIds.filter(Boolean)));
    options.setSelectedSourceIds(unique);
    const currentPrimary = options.getPrimarySourceId();
    options.setPrimarySourceId(currentPrimary && unique.includes(currentPrimary) ? currentPrimary : unique[0] ?? null);
    const profile = options.getActiveProfile();
    if (profile) {
      const currentEnabled = profile.sources.filter((source) => source.enabled).map((source) => source.id);
      const sameSelection =
        currentEnabled.length === unique.length &&
        currentEnabled.every((id) => unique.includes(id));
      if (!sameSelection) {
        void options.persistProfileUpdate(updateProfileSources(profile, unique, options.getSources()));
      }
    }
    options.setSolveResponse(null);
    options.setRawMarkers([]);
    options.setPollResults([]);
    options.setLastPollMs(null);

    if (!options.hasAnyFeedSources()) {
      options.setFeedStatus('idle');
      options.setFeedMessage(null);
      return;
    }

    options.setFeedStatus('connecting');
    options.setFeedMessage(null);
    options.feedPoller.schedule(0);
  };

  return {
    setFieldMapSelection,
    toggleSolverOutputSpace,
    setActiveSolverMode,
    setSolveSpaceEnabled,
    setPipelineTemplateId,
    setSourceInputKey,
    applySourceSelection
  };
};
