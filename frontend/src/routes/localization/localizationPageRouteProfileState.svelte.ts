import { groupLocalizationSources, selectPrimarySource } from '$lib/features/localization/page/localizationPageViewHelpers';
import { normalizePollHz, pollIntervalMs, type PollRateLimits } from '$lib/features/localization/feedPoller';
import type { LocalizationMarker } from '$lib/features/localization/viewers/localizationViewerTypes';
import type { LocalizationPoseSpace, LocalizationProfile, LocalizationSolveResponse, LocalizationSourceSampleStatus, LocalizationSolverConfig, LocalizationSolverMode, LocalizationSolverOutputs, LocalizationSolverResult } from '$lib/features/localization/localizationConfig';
import type { LocalizationPipelineSource } from '$lib/features/localization/pipelineSources';
import { computeActiveProfileColor, computeActiveSolverConfig, computeProfileIndexById, computeViewOverlaySources, computeViewProfiles, sourceStreamOutputKey } from './localizationPageRouteSupport';
import type { LocalizationPageRouteCore } from './localizationPageRouteCore.svelte';

export function createLocalizationPageRouteProfileState(core: LocalizationPageRouteCore) {
  const compatibleSources = $derived.by<LocalizationPipelineSource[]>(() => {
    const selfProfileStreamId = activeProfileSourceStreamId;
    return core.state.sources.filter((entry) => {
      if (selfProfileStreamId && entry.streamId.trim() === selfProfileStreamId) return false;
      return core.isLocalizationCompatibleSource(entry);
    });
  });

  const activeProfileSourceStreamId = $derived.by(() => {
    const profileId = (core.activeProfile.current?.id ?? core.activeProfileId.current ?? '').trim();
    return profileId ? `profile:${profileId}` : '';
  });

  const runtimeActiveProfile = $derived.by<LocalizationProfile | null>(() => {
    const profileId = (core.localizationConfig.current?.activeProfileId ?? '').trim();
    if (!profileId) return null;
    return core.profiles.current.find((profile) => profile.id === profileId && profile.enabled !== false) ?? null;
  });

  const profileFieldOrigin = $derived.by(() =>
    core.normalizeFieldOriginConfig((core.activeProfile.current ?? null)?.fieldOrigin)
  );
  const profileFieldOriginCustom = $derived.by(() => profileFieldOrigin.custom ?? { x: 0, z: 0, yawDeg: 0 });

  const sourceIdById = $derived.by<Map<string, string>>(
    () => new Map(core.state.sources.map((source) => [source.id, source.id]))
  );
  const sourceIdByStreamOutput = $derived.by<Map<string, string>>(() => {
    const map = new Map<string, string>();
    for (const source of core.state.sources) {
      const key = sourceStreamOutputKey(source.streamId, source.outputKey);
      if (!key) continue;
      map.set(key, source.id);
    }
    return map;
  });
  const compatibleSourceIdById = $derived.by<Map<string, string>>(
    () => new Map(compatibleSources.map((source) => [source.id, source.id]))
  );
  const compatibleSourceIdByStreamOutput = $derived.by<Map<string, string>>(() => {
    const map = new Map<string, string>();
    for (const source of compatibleSources) {
      const key = sourceStreamOutputKey(source.streamId, source.outputKey);
      if (!key) continue;
      map.set(key, source.id);
    }
    return map;
  });

  const resolveProfileSourceId = (
    source: { id?: string | null; streamId?: string | null; outputKey?: string | null },
    options: { compatibleOnly?: boolean } = {}
  ): string | null => {
    const compatibleOnly = options.compatibleOnly ?? false;
    const id = String(source.id ?? '').trim();
    const streamOutput = sourceStreamOutputKey(source.streamId, source.outputKey);
    const byId = compatibleOnly ? compatibleSourceIdById : sourceIdById;
    const byStreamOutput = compatibleOnly ? compatibleSourceIdByStreamOutput : sourceIdByStreamOutput;
    if (id && byId.has(id)) {
      return byId.get(id) ?? null;
    }
    if (streamOutput) {
      return byStreamOutput.get(streamOutput) ?? null;
    }
    return null;
  };

  const selectedSources = $derived.by<LocalizationPipelineSource[]>(() =>
    core.state.selectedSourceIds
      .map((id) => core.state.sources.find((entry) => entry.id === id))
      .filter((entry): entry is LocalizationPipelineSource => Boolean(entry))
  );

  const sourceWeightsById = $derived.by<Record<string, number>>(() => {
    const out: Record<string, number> = {};
    for (const source of core.activeProfile.current?.sources ?? []) {
      const weight = Number(source.weight);
      const resolvedId = resolveProfileSourceId(source);
      const nextWeight = Number.isFinite(weight) ? weight : 1;
      const id = String(source.id ?? '').trim();
      if (id) out[id] = nextWeight;
      if (resolvedId) out[resolvedId] = nextWeight;
    }
    return out;
  });

  const sourceUsedByProfilesById = $derived.by<Record<string, string[]>>(() => {
    const out: Record<string, string[]> = {};
    const activeId = core.activeProfile.current?.id ?? '';
    for (const profile of core.profiles.current) {
      if (profile.id === activeId) continue;
      const label = (profile.name ?? '').trim() || profile.id;
      for (const source of profile.sources ?? []) {
        if (!source.enabled) continue;
        const id = resolveProfileSourceId(source, { compatibleOnly: true });
        if (!id) continue;
        const list = out[id] ?? [];
        if (!list.includes(label)) {
          out[id] = [...list, label];
        }
      }
    }
    return out;
  });

  const activeSolverConfig = $derived(computeActiveSolverConfig(core.activeProfile.current ?? null, core.state.activeSolverId));
  const profileTemporalStabilization = $derived(
    core.normalizeTemporalSettings((core.activeProfile.current ?? null)?.temporalStabilization)
  );
  const activeSolverTemporalOverride = $derived(
    activeSolverConfig?.temporalStabilization
      ? core.normalizeTemporalSettings(activeSolverConfig.temporalStabilization)
      : null
  );
  const activeSolverTemporalEffective = $derived(activeSolverTemporalOverride ?? profileTemporalStabilization);
  const activeSolverRuntimeTuning = $derived(core.normalizeSolverRuntimeTuning(activeSolverConfig?.runtimeTuning));

  $effect(() => {
    const mode = profileFieldOrigin.mode;
    if (core.state.fieldOriginMode !== mode) {
      core.state.fieldOriginMode = mode;
    }
  });

  const activeSolverResult = $derived.by<LocalizationSolverResult | null>(() => {
    const solvers = core.state.solveResponse?.solvers ?? [];
    const requested = activeSolverConfig?.id ?? core.state.activeSolverId;
    if (requested) {
      return solvers.find((solver) => solver.id === requested) ?? solvers[0] ?? null;
    }
    return solvers[0] ?? null;
  });

  const activeSolverOutputs = $derived.by<LocalizationSolverOutputs | null>(() => activeSolverResult?.outputs ?? null);

  const outputsForProfile = (profile: LocalizationProfile): LocalizationSolverOutputs | null => {
    const resp =
      core.state.solveResponsesByProfile[profile.id] ??
      (profile.id === core.state.solveResponse?.profileId ? core.state.solveResponse : null);
    const solvers = resp?.solvers ?? [];
    if (profile.id === (core.activeProfile.current?.id ?? null)) {
      const requested = activeSolverConfig?.id ?? core.state.activeSolverId;
      if (requested) {
        return solvers.find((solver) => solver.id === requested)?.outputs ?? solvers[0]?.outputs ?? null;
      }
    }
    return solvers[0]?.outputs ?? null;
  };

  const enabledSourcesForProfile = (profile: LocalizationProfile): LocalizationPipelineSource[] => {
    const enabledIds = new Set((profile.sources ?? []).filter((source) => source.enabled).map((source) => source.id));
    return core.state.sources.filter((source) => enabledIds.has(source.id));
  };

  $effect(() => {
    const profile = core.activeProfile.current;
    const solvers = profile?.solvers ?? [];
    if (solvers.length === 0) {
      core.state.activeSolverId = '';
      return;
    }
    if (core.state.activeSolverId && solvers.some((solver) => solver.id === core.state.activeSolverId)) {
      return;
    }
    core.state.activeSolverId = solvers[0]?.id ?? '';
  });

  const profileIndexById = $derived(computeProfileIndexById(core.profiles.current));
  const activeProfileColor = $derived(
    computeActiveProfileColor(core.activeProfile.current ?? null, core.profiles.current, profileIndexById, core.PROFILE_COLORS)
  );
  const viewProfiles = $derived(computeViewProfiles(core.profiles.current));
  const filteredProfiles = $derived.by<LocalizationProfile[]>(() => {
    const q = core.state.profileSearch.trim().toLowerCase();
    if (!q) return core.profiles.current;
    return core.profiles.current.filter((profile) => {
      const name = (profile.name ?? '').toLowerCase();
      const id = (profile.id ?? '').toLowerCase();
      return name.includes(q) || id.includes(q);
    });
  });

  const viewerAccentColor = $derived.by<string | null>(() => {
    if (viewProfiles.length === 1) {
      const id = viewProfiles[0]?.id ?? '';
      return id ? core.PROFILE_COLORS[profileIndexById.get(id) ?? 0] ?? activeProfileColor : activeProfileColor;
    }
    return activeProfileColor;
  });

  const viewOverlaySources = $derived(computeViewOverlaySources(core.profiles.current, core.state.sources));
  const viewerSourcePool = $derived.by<LocalizationPipelineSource[]>(() =>
    hasAnyViewsEnabled ? viewOverlaySources : selectedSources
  );
  const viewProfilesWithSources = $derived.by<LocalizationProfile[]>(() =>
    viewProfiles.filter((profile) => profile.sources.some((source) => source.enabled))
  );
  const activeHasSources = $derived.by(() => (runtimeActiveProfile?.sources ?? []).some((source) => source.enabled));
  const hasAnyFeedSources = $derived.by(() => activeHasSources || viewProfilesWithSources.length > 0);
  const hasAnyViewsEnabled = $derived.by(() => viewProfiles.length > 0);

  const supportedSolverModes = $derived.by<LocalizationSolverMode[]>(() => {
    const supported = core.state.localizationCapabilities?.constraints?.supportedSolverModes ?? [];
    const seen = new Set<string>();
    const ordered: LocalizationSolverMode[] = [];
    for (const mode of supported) {
      const normalized = String(mode ?? '').trim();
      if (!normalized || seen.has(normalized)) continue;
      seen.add(normalized);
      ordered.push(mode);
    }
    if (ordered.length > 0) return ordered;
    const configuredModes = core.profiles.current.flatMap((profile) => profile.solvers.map((solver) => solver.mode));
    for (const mode of configuredModes) {
      const normalized = String(mode ?? '').trim();
      if (!normalized || seen.has(normalized)) continue;
      seen.add(normalized);
      ordered.push(mode);
    }
    return ordered;
  });

  const supportedPoseSpaces = $derived.by<LocalizationPoseSpace[]>(() => {
    const supported = core.state.localizationCapabilities?.constraints?.supportedPoseSpaces ?? [];
    const seen = new Set<string>();
    const ordered: LocalizationPoseSpace[] = [];
    for (const space of supported) {
      const normalized = String(space ?? '').trim();
      if (!normalized || seen.has(normalized)) continue;
      seen.add(normalized);
      ordered.push(space);
    }
    if (ordered.length > 0) return ordered;
    const configuredSpaces = core.profiles.current.flatMap((profile) =>
      profile.solvers.flatMap((solver) => (solver.outputSpaces ?? []).filter(Boolean))
    );
    for (const space of configuredSpaces) {
      const normalized = String(space ?? '').trim();
      if (!normalized || seen.has(normalized)) continue;
      seen.add(normalized);
      ordered.push(space);
    }
    return ordered;
  });

  const supportedPoseSpaceSet = $derived.by(() => new Set(supportedPoseSpaces));
  const maxMapUploadBytes = $derived.by<number | null>(() => {
    const raw = Number(core.state.localizationCapabilities?.constraints?.maxMapUploadBytes ?? Number.NaN);
    if (!Number.isFinite(raw) || raw <= 0) return null;
    return Math.trunc(raw);
  });
  const pollRateLimits = $derived.by<PollRateLimits | null>(() => {
    const minRaw = Number(core.state.localizationCapabilities?.constraints?.minPollHz ?? Number.NaN);
    const maxRaw = Number(core.state.localizationCapabilities?.constraints?.maxPollHz ?? Number.NaN);
    const defaultRaw = Number(core.state.localizationCapabilities?.constraints?.defaultPollHz ?? Number.NaN);
    if (!Number.isFinite(minRaw) || !Number.isFinite(maxRaw) || !Number.isFinite(defaultRaw)) return null;
    const minHz = Math.max(1, Math.floor(minRaw));
    const maxHz = Math.max(minHz, Math.floor(maxRaw));
    const defaultHz = Math.min(maxHz, Math.max(minHz, Math.floor(defaultRaw)));
    return { minHz, maxHz, defaultHz };
  });
  const pollHzStep = $derived.by<number>(() => {
    const raw = Number(core.state.localizationCapabilities?.constraints?.pollStepHz ?? Number.NaN);
    return Number.isFinite(raw) && raw > 0 ? Math.floor(raw) : 1;
  });
  const pollHzMin = $derived.by<number>(() => pollRateLimits?.minHz ?? 1);
  const pollHzMax = $derived.by<number>(() => pollRateLimits?.maxHz ?? Math.max(1, pollHzMin));
  $effect(() => {
    const normalized = normalizePollHz(core.state.pollHz, pollRateLimits);
    if (normalized !== core.state.pollHz) core.state.pollHz = normalized;
  });

  const solverOutputSpaces = $derived.by<LocalizationPoseSpace[]>(() => activeSolverConfig?.outputSpaces ?? []);
  const supportedSolvePoseSpaces = $derived.by<LocalizationPoseSpace[]>(() =>
    core.SOLVE_POSE_SPACES.filter((space) => supportedPoseSpaceSet.has(space))
  );
  const solvePoseSpaces = $derived.by<LocalizationPoseSpace[]>(() =>
    supportedSolvePoseSpaces.filter((space) => solverOutputSpaces.includes(space))
  );
  const derivedPoseSpaces = $derived.by<LocalizationPoseSpace[]>(() => {
    const derived = new Set<LocalizationPoseSpace>();
    for (const space of solverOutputSpaces) {
      for (const next of core.DERIVED_POSE_SPACES[space] ?? []) {
        derived.add(next);
      }
    }
    return Array.from(derived.values()).filter((space) => supportedPoseSpaceSet.has(space));
  });
  const availableCoordinateSpaces = $derived.by<LocalizationPoseSpace[]>(() => {
    const next: LocalizationPoseSpace[] = [];
    const seen = new Set<string>();
    for (const space of solverOutputSpaces.concat(derivedPoseSpaces)) {
      if (!supportedPoseSpaceSet.has(space) || seen.has(space)) continue;
      seen.add(space);
      next.push(space);
    }
    return next;
  });

  const profileSupportedSpacesById = $derived.by<Record<string, LocalizationPoseSpace[]>>(() => {
    const out: Record<string, LocalizationPoseSpace[]> = {};
    for (const profile of core.profiles.current) {
      const outputSpaces = (profile.solvers ?? [])
        .flatMap((solver) => solver.outputSpaces ?? [])
        .filter((space) => supportedPoseSpaceSet.has(space));
      const supported = new Set<LocalizationPoseSpace>(outputSpaces);
      if (!profile.fieldMapId) {
        supported.delete('robot_in_field');
        supported.delete('camera_in_field');
      }
      out[profile.id] = Array.from(supported.values());
    }
    return out;
  });

  const shouldPollFieldPosesOnly = (): boolean =>
    (core.state.coordinateSpace === 'camera_in_field' || core.state.coordinateSpace === 'robot_in_field') &&
    !core.state.showOutputsOverlay &&
    !core.state.showTagLines;

  const { runFeedPoll } = core.createLocalizationFeedRuntime({
    getActiveProfile: () => runtimeActiveProfile ?? null,
    getHasAnyFeedSources: () => hasAnyFeedSources,
    getActiveHasSources: () => activeHasSources,
    getViewProfilesWithSources: () => viewProfilesWithSources,
    fetchLocalizationSolve: (profileId, signal) =>
      core.localizationProfiles.persist
        ? import('$lib/features/localization/localizationConfig').then(({ fetchLocalizationSolve }) =>
            fetchLocalizationSolve(profileId, signal, { fieldPosesOnly: shouldPollFieldPosesOnly() })
          )
        : Promise.reject(new Error('Localization solve unavailable')),
    setSolveResponsesByProfile: (next) => {
      core.state.solveResponsesByProfile = next;
    },
    setSolveResponse: (next) => {
      core.state.solveResponse = next;
    },
    setPollResults: (next) => {
      core.state.pollResults = next;
    },
    setFeedStatus: (next) => {
      core.state.feedStatus = next;
    },
    setFeedMessage: (next) => {
      core.state.feedMessage = next;
    },
    setLastPollMs: (next) => {
      core.state.lastPollMs = next;
    }
  });

  const feedPoller = core.createFeedPoller({
    getIntervalMs: () => pollIntervalMs(core.state.pollHz, pollRateLimits),
    hasSources: () => hasAnyFeedSources,
    onPoll: runFeedPoll
  });

  const localizationActions = core.createLocalizationPageActions({
    getActiveProfile: () => core.activeProfile.current ?? null,
    getActiveSolverConfig: () => activeSolverConfig,
    getSolvePoseSpaces: () => solvePoseSpaces,
    getSupportedPoseSpaces: () => supportedPoseSpaces,
    setFieldMapSelection: (next) => {
      core.state.fieldMapSelection = next;
    },
    assignMapToSelectedField: core.assignMapToSelectedField,
    persistProfileUpdate: core.persistProfileUpdate,
    feedPoller,
    getSelectedSourceIds: () => core.state.selectedSourceIds,
    setSelectedSourceIds: (next) => {
      core.state.selectedSourceIds = next;
    },
    getPrimarySourceId: () => core.state.primarySourceId,
    setPrimarySourceId: (next) => {
      core.state.primarySourceId = next;
    },
    getSources: () => core.state.sources,
    setSolveResponse: (next) => {
      core.state.solveResponse = next as LocalizationSolveResponse | null;
    },
    setRawMarkers: (next) => {
      core.state.rawMarkers = next as LocalizationMarker[];
    },
    setPollResults: (next) => {
      core.state.pollResults = next as LocalizationSourceSampleStatus[];
    },
    setLastPollMs: (next) => {
      core.state.lastPollMs = next;
    },
    hasAnyFeedSources: () => hasAnyFeedSources,
    setFeedStatus: (next) => {
      core.state.feedStatus = next;
    },
    setFeedMessage: (next) => {
      core.state.feedMessage = next;
    }
  });
  core.setApplySourceSelectionImpl(localizationActions.applySourceSelection);

  const setActiveSolverMode = (mode: LocalizationSolverMode): void => {
    if (!supportedSolverModes.includes(mode)) return;
    localizationActions.setActiveSolverMode(mode);
  };

  const setActiveSolverIdForUi = (nextId: string): void => {
    core.state.activeSolverId = nextId;
  };

  const commitSolverName = (): void => {
    const profile = core.activeProfile.current;
    const solver = activeSolverConfig;
    if (!profile || !solver) return;
    const nextName = core.state.solverNameInput.trim();
    if (!nextName || nextName === solver.name) return;
    const nextSolvers = profile.solvers.map((entry) => (entry.id === solver.id ? { ...entry, name: nextName } : entry));
    void core.persistProfileUpdate({ ...profile, solvers: nextSolvers });
  };

  const addSolver = (): void => {
    const profile = core.activeProfile.current;
    if (!profile) return;
    const existing = new Set(profile.solvers.map((solver) => solver.id));
    const base = 'group';
    let id = `${base}-${Date.now().toString(36)}`;
    let counter = 0;
    while (existing.has(id)) {
      counter += 1;
      id = `${base}-${Date.now().toString(36)}-${counter}`;
    }
    const template = activeSolverConfig ?? profile.solvers[0] ?? null;
    const capabilityDefaultMode = core.state.localizationCapabilities?.defaults?.defaultSolverMode ?? null;
    const modeCandidates: Array<LocalizationSolverMode | null | undefined> = [
      template?.mode,
      capabilityDefaultMode,
      supportedSolverModes[0],
      profile.solvers[0]?.mode
    ];
    const supportedModeSet = new Set(supportedSolverModes);
    const mode =
      modeCandidates.find(
        (candidate): candidate is LocalizationSolverMode =>
          Boolean(candidate) && (supportedModeSet.size === 0 || supportedModeSet.has(candidate))
      ) ?? null;
    if (!mode) {
      core.toaster.error({
        title: 'Unable to add solver',
        description: 'No backend-supported solver mode is available.'
      });
      return;
    }
    const capabilityDefaultOutputSpaces = (core.state.localizationCapabilities?.defaults?.defaultSolverOutputSpaces ?? []).filter((space) =>
      supportedPoseSpaceSet.has(space)
    );
    const templateOutputSpaces = (template?.outputSpaces ?? []).filter((space) => supportedPoseSpaceSet.has(space));
    const fallbackSolveSpaces = supportedSolvePoseSpaces.slice(0, 2);
    const outputSpaces = templateOutputSpaces.length
      ? [...templateOutputSpaces]
      : capabilityDefaultOutputSpaces.length
        ? [...capabilityDefaultOutputSpaces]
        : fallbackSolveSpaces.length
          ? [...fallbackSolveSpaces]
          : [];
    const nextSolver: LocalizationSolverConfig = {
      id,
      name: `Group ${profile.solvers.length + 1}`,
      mode,
      outputSpaces,
      sourceIds: [],
      color: null,
      runtimeTuning: core.normalizeSolverRuntimeTuning(template?.runtimeTuning),
      temporalStabilization: template?.temporalStabilization
        ? core.normalizeTemporalSettings(template.temporalStabilization)
        : null
    };
    void core.persistProfileUpdate({ ...profile, solvers: [...profile.solvers, nextSolver] });
    core.state.activeSolverId = id;
  };

  const removeActiveSolver = (): void => {
    const profile = core.activeProfile.current;
    const solver = activeSolverConfig;
    if (!profile || !solver || profile.solvers.length <= 1) return;
    const nextSolvers = profile.solvers.filter((entry) => entry.id !== solver.id);
    void core.persistProfileUpdate({ ...profile, solvers: nextSolvers });
    core.state.activeSolverId = nextSolvers[0]?.id ?? '';
  };

  const setActiveSolverSourceIds = (nextIds: string[]): void => {
    const profile = core.activeProfile.current;
    const solver = activeSolverConfig;
    if (!profile || !solver) return;
    const unique = Array.from(new Set(nextIds.map((id) => id.trim()).filter(Boolean)));
    const current = solver.sourceIds ?? [];
    const same = current.length === unique.length && current.every((id) => unique.includes(id));
    if (same) return;
    const nextSolvers = profile.solvers.map((entry) => (entry.id === solver.id ? { ...entry, sourceIds: unique } : entry));
    void core.persistProfileUpdate({ ...profile, solvers: nextSolvers });
  };

  const setActiveSolverUseAllSources = (useAll: boolean): void => {
    if (useAll) {
      setActiveSolverSourceIds([]);
      return;
    }
    setActiveSolverSourceIds([...core.state.selectedSourceIds]);
  };

  const toggleActiveSolverSource = (sourceId: string, enabled: boolean): void => {
    const solver = activeSolverConfig;
    if (!solver) return;
    const current = solver.sourceIds ?? [];
    const expanded = current.length === 0 ? [...core.state.selectedSourceIds] : [...current];
    const next = enabled ? Array.from(new Set([...expanded, sourceId])) : expanded.filter((id) => id !== sourceId);
    setActiveSolverSourceIds(next);
  };

  const setSourceWeight = (sourceId: string, rawValue: string): void => {
    const profile = core.activeProfile.current;
    if (!profile) return;
    const parsed = Number(rawValue);
    if (!Number.isFinite(parsed)) return;
    const nextWeight = parsed;
    const nextSources = profile.sources.map((source) =>
      source.id === sourceId ? { ...source, weight: nextWeight } : source
    );
    const hadSource = nextSources.some((source) => source.id === sourceId);
    if (!hadSource) {
      const available = core.state.sources.find((source) => source.id === sourceId);
      if (!available) return;
      nextSources.push({
        id: available.id,
        streamId: available.streamId,
        outputKey: available.outputKey,
        cameraUid: available.cameraUid,
        poseSpace: null,
        inputKey: null,
        enabled: core.state.selectedSourceIds.includes(sourceId),
        weight: nextWeight
      });
    }
    const currentWeight = profile.sources.find((source) => source.id === sourceId)?.weight;
    if (currentWeight != null && Math.abs(currentWeight - nextWeight) < 1e-6) return;
    void core.persistProfileUpdate({ ...profile, sources: nextSources });
  };

  const { toggleSourceGroup, toggleSource } = core.createLocalizationSourceSelection({
    getOpenSourceGroups: () => core.state.openSourceGroups,
    setOpenSourceGroups: (next) => {
      core.state.openSourceGroups = next;
    },
    applySourceSelection: core.applySourceSelection,
    getSelectedSourceIds: () => core.state.selectedSourceIds
  });

  $effect(() => {
    const profile = core.activeProfile.current;
    if (!profile) {
      core.state.selectedSourceIds = [];
      return;
    }
    if (!core.activeProfileId.current || core.activeProfileId.current !== profile.id) {
      core.activeProfileId.current = profile.id;
    }
    const selfProfileStreamId = `profile:${profile.id}`;
    const enabled = Array.from(
      new Set(
        profile.sources
          .filter((source) => source.enabled)
          .map((source) => resolveProfileSourceId(source))
          .filter((id): id is string => Boolean(id))
          .filter((id) => {
            const resolved = core.state.sources.find((entry) => entry.id === id);
            return Boolean(resolved) && resolved.streamId.trim() !== selfProfileStreamId;
          })
      )
    );
    core.state.selectedSourceIds = enabled;
    core.state.primarySourceId =
      core.state.primarySourceId && enabled.includes(core.state.primarySourceId)
        ? core.state.primarySourceId
        : enabled[0] ?? null;
  });

  $effect(() => {
    const profile = core.activeProfile.current;
    if (!profile) {
      core.state.profileNameInput = '';
      core.state.profileNameTargetId = null;
      return;
    }
    if (core.state.profileNameTargetId !== profile.id) {
      core.state.profileNameInput = profile.name ?? '';
      core.state.profileNameTargetId = profile.id;
    }
  });

  $effect(() => {
    const profile = core.activeProfile.current;
    const solver = activeSolverConfig;
    if (!profile || !solver) {
      core.state.solverNameInput = '';
      core.state.solverNameTargetKey = null;
      return;
    }
    const key = `${profile.id}:${solver.id}`;
    if (core.state.solverNameTargetKey !== key) {
      core.state.solverNameInput = solver.name ?? '';
      core.state.solverNameTargetKey = key;
    }
  });

  $effect(() => {
    const profile = core.activeProfile.current;
    if (!profile) {
      core.state.tagSizeInput = '';
      core.state.tagSizeTargetId = null;
      core.state.tagSizeError = null;
      return;
    }
    if (core.state.tagSizeTargetId !== profile.id) {
      core.state.tagSizeInput =
        typeof profile.tagSizeM === 'number' && Number.isFinite(profile.tagSizeM)
          ? core.formatMeters(profile.tagSizeM, 'm', 4)
          : '';
      core.state.tagSizeTargetId = profile.id;
      core.state.tagSizeError = null;
    }
  });

  $effect(() => {
    const profile = core.activeProfile.current;
    if (!profile) {
      core.state.excludedTagIdsInput = '';
      core.state.excludedTagIdsTargetId = null;
      core.state.excludedTagIdsError = null;
      return;
    }
    if (core.state.excludedTagIdsTargetId !== profile.id) {
      const nextIds = Array.isArray(profile.excludedTagIds) ? profile.excludedTagIds : [];
      core.state.excludedTagIdsInput = nextIds.join(', ');
      core.state.excludedTagIdsTargetId = profile.id;
      core.state.excludedTagIdsError = null;
    }
  });

  const groupedSources = $derived.by(() => groupLocalizationSources(compatibleSources));
  const primarySource = $derived.by(() => selectPrimarySource(core.state.primarySourceId, core.state.sources, selectedSources));
  const primaryCameraKey = $derived.by(() => core.cameraKeyForSource(primarySource));
  const api = {
    addSolver,
    commitSolverName,
    enabledSourcesForProfile,
    feedPoller,
    outputsForProfile,
    removeActiveSolver,
    resolveProfileSourceId,
    setFieldMapSelection: localizationActions.setFieldMapSelection,
    setActiveSolverIdForUi,
    setActiveSolverMode,
    setActiveSolverUseAllSources,
    setSourceWeight,
    toggleActiveSolverSource,
    toggleSource,
    toggleSourceGroup,
    get activeHasSources() {
      return activeHasSources;
    },
    get activeProfileColor() {
      return activeProfileColor;
    },
    get activeSolverConfig() {
      return activeSolverConfig;
    },
    get activeSolverOutputs() {
      return activeSolverOutputs;
    },
    get activeSolverResult() {
      return activeSolverResult;
    },
    get activeSolverRuntimeTuning() {
      return activeSolverRuntimeTuning;
    },
    get activeSolverTemporalEffective() {
      return activeSolverTemporalEffective;
    },
    get activeSolverTemporalOverride() {
      return activeSolverTemporalOverride;
    },
    get availableCoordinateSpaces() {
      return availableCoordinateSpaces;
    },
    get compatibleSources() {
      return compatibleSources;
    },
    get derivedPoseSpaces() {
      return derivedPoseSpaces;
    },
    get filteredProfiles() {
      return filteredProfiles;
    },
    get groupedSources() {
      return groupedSources;
    },
    get hasAnyFeedSources() {
      return hasAnyFeedSources;
    },
    get hasAnyViewsEnabled() {
      return hasAnyViewsEnabled;
    },
    get maxMapUploadBytes() {
      return maxMapUploadBytes;
    },
    get pollHzMax() {
      return pollHzMax;
    },
    get pollHzMin() {
      return pollHzMin;
    },
    get pollHzStep() {
      return pollHzStep;
    },
    get pollRateLimits() {
      return pollRateLimits;
    },
    get primaryCameraKey() {
      return primaryCameraKey;
    },
    get primarySource() {
      return primarySource;
    },
    get profileFieldOrigin() {
      return profileFieldOrigin;
    },
    get profileFieldOriginCustom() {
      return profileFieldOriginCustom;
    },
    get profileIndexById() {
      return profileIndexById;
    },
    get profileSupportedSpacesById() {
      return profileSupportedSpacesById;
    },
    get profileTemporalStabilization() {
      return profileTemporalStabilization;
    },
    get runtimeActiveProfile() {
      return runtimeActiveProfile;
    },
    get selectedSources() {
      return selectedSources;
    },
    get solvePoseSpaces() {
      return solvePoseSpaces;
    },
    get sourceUsedByProfilesById() {
      return sourceUsedByProfilesById;
    },
    get sourceWeightsById() {
      return sourceWeightsById;
    },
    get supportedPoseSpaceSet() {
      return supportedPoseSpaceSet;
    },
    get supportedPoseSpaces() {
      return supportedPoseSpaces;
    },
    get supportedSolvePoseSpaces() {
      return supportedSolvePoseSpaces;
    },
    get supportedSolverModes() {
      return supportedSolverModes;
    },
    get viewOverlaySources() {
      return viewOverlaySources;
    },
    get viewerAccentColor() {
      return viewerAccentColor;
    },
    get viewerSourcePool() {
      return viewerSourcePool;
    },
    get viewProfiles() {
      return viewProfiles;
    },
    get viewProfilesWithSources() {
      return viewProfilesWithSources;
    }
  };

  return api;
}

export type LocalizationPageRouteProfileState = ReturnType<typeof createLocalizationPageRouteProfileState>;
