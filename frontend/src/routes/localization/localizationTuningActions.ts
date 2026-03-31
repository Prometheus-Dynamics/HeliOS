import {
  DEFAULT_FIELD_ORIGIN,
  DEFAULT_SOLVER_RUNTIME_TUNING,
  DEFAULT_TEMPORAL_STABILIZATION,
  type LocalizationCustomFieldOrigin,
  type LocalizationFieldOriginConfig,
  type LocalizationFieldOriginMode,
  type LocalizationProfile,
  type LocalizationSolverConfig,
  type LocalizationSolverRuntimeTuningConfig,
  type LocalizationTemporalStabilizationConfig
} from '$lib/features/localization/localizationConfig';

type LocalizationTuningActionsDeps = {
  getActiveProfile: () => LocalizationProfile | null;
  getActiveSolver: () => LocalizationSolverConfig | null;
  persistProfileUpdate: (profile: LocalizationProfile) => Promise<void> | void;
};

const temporalFieldParsers = {
  singleTagTranslationAlpha: {},
  singleTagRotationAlpha: {},
  multiTagTranslationAlpha: {},
  multiTagRotationAlpha: {},
  maxTranslationJumpM: {},
  maxRotationJumpDeg: {},
  reanchorRejectWindowMs: { integer: true }
} as const;

type TemporalNumericField = keyof typeof temporalFieldParsers;

const solverRuntimeFieldParsers = {
  minObservationWeight: {},
  minSingleTagSolveWeight: {},
  minMultiTagTotalWeight: {},
  minMultiTagEffectiveCount: {},
  weakSingleTagMargin: {},
  coplanarHeightDeltaM: {},
  severeObservedHeightDeltaM: {},
  moderateObservedHeightDeltaM: {},
  mildObservedHeightDeltaM: {},
  severePenalty: {},
  moderatePenalty: {},
  mildPenalty: {},
  dtScaleMin: {},
  dtScaleMax: {},
  switchedSingleTagMaxTranslationJumpM: {},
  switchedSingleTagMaxRotationJumpDeg: {},
  droppedMultiToSingleMaxTranslationJumpM: {},
  droppedMultiToSingleMaxRotationJumpDeg: {},
  switchedSingleTagRejectWindowScale: {},
  switchedSingleTagRejectWindowMinMs: { integer: true },
  droppedMultiToSingleRejectWindowScale: {},
  droppedMultiToSingleRejectWindowMinMs: { integer: true },
  switchedSingleTagGainDamp: {},
  switchedSingleTagMinTranslationGain: {},
  switchedSingleTagMinRotationGain: {},
  droppedMultiToSingleGainDamp: {},
  droppedMultiToSingleMinTranslationGain: {},
  droppedMultiToSingleMinRotationGain: {}
} as const;

type SolverRuntimeNumericField = keyof typeof solverRuntimeFieldParsers;

const isTemporalNumericField = (field: string): field is TemporalNumericField =>
  Object.prototype.hasOwnProperty.call(temporalFieldParsers, field);

const isSolverRuntimeNumericField = (field: string): field is SolverRuntimeNumericField =>
  Object.prototype.hasOwnProperty.call(solverRuntimeFieldParsers, field);

export const normalizeFieldOriginConfig = (
  value?: LocalizationFieldOriginConfig | null
): LocalizationFieldOriginConfig => {
  const fallback = { ...DEFAULT_FIELD_ORIGIN };
  if (!value || typeof value !== 'object') return fallback;
  const mode = value.mode ?? fallback.mode;
  const validMode: LocalizationFieldOriginMode =
    mode === 'center' || mode === 'blue' || mode === 'red' || mode === 'custom'
      ? mode
      : fallback.mode;
  const custom = value.custom;
  const hasFiniteCustom =
    custom &&
    Number.isFinite(custom.x) &&
    Number.isFinite(custom.z) &&
    Number.isFinite(custom.yawDeg);
  return {
    mode: validMode,
    custom: hasFiniteCustom
      ? {
          x: Number(custom.x),
          z: Number(custom.z),
          yawDeg: Number(custom.yawDeg)
        }
      : null
  };
};

export const normalizeTemporalSettings = (
  settings?: LocalizationTemporalStabilizationConfig | null
): LocalizationTemporalStabilizationConfig => {
  const merged = { ...DEFAULT_TEMPORAL_STABILIZATION, ...(settings ?? {}) };
  const numberOrDefault = (value: number, fallback: number): number =>
    Number.isFinite(value) ? Number(value) : fallback;
  return {
    enabled: Boolean(merged.enabled),
    singleTagTranslationAlpha: numberOrDefault(
      merged.singleTagTranslationAlpha,
      DEFAULT_TEMPORAL_STABILIZATION.singleTagTranslationAlpha
    ),
    singleTagRotationAlpha: numberOrDefault(
      merged.singleTagRotationAlpha,
      DEFAULT_TEMPORAL_STABILIZATION.singleTagRotationAlpha
    ),
    multiTagTranslationAlpha: numberOrDefault(
      merged.multiTagTranslationAlpha,
      DEFAULT_TEMPORAL_STABILIZATION.multiTagTranslationAlpha
    ),
    multiTagRotationAlpha: numberOrDefault(
      merged.multiTagRotationAlpha,
      DEFAULT_TEMPORAL_STABILIZATION.multiTagRotationAlpha
    ),
    maxTranslationJumpM: numberOrDefault(
      merged.maxTranslationJumpM,
      DEFAULT_TEMPORAL_STABILIZATION.maxTranslationJumpM
    ),
    maxRotationJumpDeg: numberOrDefault(
      merged.maxRotationJumpDeg,
      DEFAULT_TEMPORAL_STABILIZATION.maxRotationJumpDeg
    ),
    reanchorRejectWindowMs: Math.round(
      numberOrDefault(
        merged.reanchorRejectWindowMs,
        DEFAULT_TEMPORAL_STABILIZATION.reanchorRejectWindowMs
      )
    )
  };
};

export const normalizeSolverRuntimeTuning = (
  settings?: LocalizationSolverRuntimeTuningConfig | null
): LocalizationSolverRuntimeTuningConfig => {
  const merged = { ...DEFAULT_SOLVER_RUNTIME_TUNING, ...(settings ?? {}) };
  const numberOrDefault = (value: number, fallback: number): number =>
    Number.isFinite(value) ? Number(value) : fallback;
  return {
    minObservationWeight: numberOrDefault(
      merged.minObservationWeight,
      DEFAULT_SOLVER_RUNTIME_TUNING.minObservationWeight
    ),
    minSingleTagSolveWeight: numberOrDefault(
      merged.minSingleTagSolveWeight,
      DEFAULT_SOLVER_RUNTIME_TUNING.minSingleTagSolveWeight
    ),
    minMultiTagTotalWeight: numberOrDefault(
      merged.minMultiTagTotalWeight,
      DEFAULT_SOLVER_RUNTIME_TUNING.minMultiTagTotalWeight
    ),
    minMultiTagEffectiveCount: numberOrDefault(
      merged.minMultiTagEffectiveCount,
      DEFAULT_SOLVER_RUNTIME_TUNING.minMultiTagEffectiveCount
    ),
    weakSingleTagMargin: numberOrDefault(
      merged.weakSingleTagMargin,
      DEFAULT_SOLVER_RUNTIME_TUNING.weakSingleTagMargin
    ),
    coplanarHeightDeltaM: numberOrDefault(
      merged.coplanarHeightDeltaM,
      DEFAULT_SOLVER_RUNTIME_TUNING.coplanarHeightDeltaM
    ),
    severeObservedHeightDeltaM: numberOrDefault(
      merged.severeObservedHeightDeltaM,
      DEFAULT_SOLVER_RUNTIME_TUNING.severeObservedHeightDeltaM
    ),
    moderateObservedHeightDeltaM: numberOrDefault(
      merged.moderateObservedHeightDeltaM,
      DEFAULT_SOLVER_RUNTIME_TUNING.moderateObservedHeightDeltaM
    ),
    mildObservedHeightDeltaM: numberOrDefault(
      merged.mildObservedHeightDeltaM,
      DEFAULT_SOLVER_RUNTIME_TUNING.mildObservedHeightDeltaM
    ),
    severePenalty: numberOrDefault(
      merged.severePenalty,
      DEFAULT_SOLVER_RUNTIME_TUNING.severePenalty
    ),
    moderatePenalty: numberOrDefault(
      merged.moderatePenalty,
      DEFAULT_SOLVER_RUNTIME_TUNING.moderatePenalty
    ),
    mildPenalty: numberOrDefault(merged.mildPenalty, DEFAULT_SOLVER_RUNTIME_TUNING.mildPenalty),
    dtScaleMin: numberOrDefault(merged.dtScaleMin, DEFAULT_SOLVER_RUNTIME_TUNING.dtScaleMin),
    dtScaleMax: numberOrDefault(merged.dtScaleMax, DEFAULT_SOLVER_RUNTIME_TUNING.dtScaleMax),
    switchedSingleTagMaxTranslationJumpM: numberOrDefault(
      merged.switchedSingleTagMaxTranslationJumpM,
      DEFAULT_SOLVER_RUNTIME_TUNING.switchedSingleTagMaxTranslationJumpM
    ),
    switchedSingleTagMaxRotationJumpDeg: numberOrDefault(
      merged.switchedSingleTagMaxRotationJumpDeg,
      DEFAULT_SOLVER_RUNTIME_TUNING.switchedSingleTagMaxRotationJumpDeg
    ),
    droppedMultiToSingleMaxTranslationJumpM: numberOrDefault(
      merged.droppedMultiToSingleMaxTranslationJumpM,
      DEFAULT_SOLVER_RUNTIME_TUNING.droppedMultiToSingleMaxTranslationJumpM
    ),
    droppedMultiToSingleMaxRotationJumpDeg: numberOrDefault(
      merged.droppedMultiToSingleMaxRotationJumpDeg,
      DEFAULT_SOLVER_RUNTIME_TUNING.droppedMultiToSingleMaxRotationJumpDeg
    ),
    switchedSingleTagRejectWindowScale: numberOrDefault(
      merged.switchedSingleTagRejectWindowScale,
      DEFAULT_SOLVER_RUNTIME_TUNING.switchedSingleTagRejectWindowScale
    ),
    switchedSingleTagRejectWindowMinMs: Math.round(
      numberOrDefault(
        merged.switchedSingleTagRejectWindowMinMs,
        DEFAULT_SOLVER_RUNTIME_TUNING.switchedSingleTagRejectWindowMinMs
      )
    ),
    droppedMultiToSingleRejectWindowScale: numberOrDefault(
      merged.droppedMultiToSingleRejectWindowScale,
      DEFAULT_SOLVER_RUNTIME_TUNING.droppedMultiToSingleRejectWindowScale
    ),
    droppedMultiToSingleRejectWindowMinMs: Math.round(
      numberOrDefault(
        merged.droppedMultiToSingleRejectWindowMinMs,
        DEFAULT_SOLVER_RUNTIME_TUNING.droppedMultiToSingleRejectWindowMinMs
      )
    ),
    switchedSingleTagGainDamp: numberOrDefault(
      merged.switchedSingleTagGainDamp,
      DEFAULT_SOLVER_RUNTIME_TUNING.switchedSingleTagGainDamp
    ),
    switchedSingleTagMinTranslationGain: numberOrDefault(
      merged.switchedSingleTagMinTranslationGain,
      DEFAULT_SOLVER_RUNTIME_TUNING.switchedSingleTagMinTranslationGain
    ),
    switchedSingleTagMinRotationGain: numberOrDefault(
      merged.switchedSingleTagMinRotationGain,
      DEFAULT_SOLVER_RUNTIME_TUNING.switchedSingleTagMinRotationGain
    ),
    droppedMultiToSingleGainDamp: numberOrDefault(
      merged.droppedMultiToSingleGainDamp,
      DEFAULT_SOLVER_RUNTIME_TUNING.droppedMultiToSingleGainDamp
    ),
    droppedMultiToSingleMinTranslationGain: numberOrDefault(
      merged.droppedMultiToSingleMinTranslationGain,
      DEFAULT_SOLVER_RUNTIME_TUNING.droppedMultiToSingleMinTranslationGain
    ),
    droppedMultiToSingleMinRotationGain: numberOrDefault(
      merged.droppedMultiToSingleMinRotationGain,
      DEFAULT_SOLVER_RUNTIME_TUNING.droppedMultiToSingleMinRotationGain
    )
  };
};

const parseTemporalNumericValue = (field: TemporalNumericField, rawValue: string): number | null => {
  const parsed = Number(rawValue);
  if (!Number.isFinite(parsed)) return null;
  const parser = temporalFieldParsers[field];
  const integer = 'integer' in parser ? Boolean(parser.integer) : false;
  return integer ? Math.round(parsed) : parsed;
};

const parseSolverRuntimeNumericValue = (
  field: SolverRuntimeNumericField,
  rawValue: string
): number | null => {
  const parsed = Number(rawValue);
  if (!Number.isFinite(parsed)) return null;
  const parser = solverRuntimeFieldParsers[field];
  const integer = 'integer' in parser ? Boolean(parser.integer) : false;
  return integer ? Math.round(parsed) : parsed;
};

export const createLocalizationTuningActions = (deps: LocalizationTuningActionsDeps) => {
  const persistActiveProfile = (buildNext: (profile: LocalizationProfile) => LocalizationProfile): void => {
    const profile = deps.getActiveProfile();
    if (!profile) return;
    void deps.persistProfileUpdate(buildNext(profile));
  };

  const setProfileFieldOriginMode = (mode: LocalizationFieldOriginMode): void => {
    persistActiveProfile((profile) => {
      const current = normalizeFieldOriginConfig(profile.fieldOrigin);
      const nextCustom =
        mode === 'custom'
          ? current.custom ?? { x: 0, z: 0, yawDeg: 0 }
          : current.custom ?? null;
      const sameCustom =
        (current.custom == null && nextCustom == null) ||
        (current.custom != null &&
          nextCustom != null &&
          current.custom.x === nextCustom.x &&
          current.custom.z === nextCustom.z &&
          current.custom.yawDeg === nextCustom.yawDeg);
      if (current.mode === mode && sameCustom) return profile;
      return {
        ...profile,
        fieldOrigin: { mode, custom: nextCustom }
      };
    });
  };

  const setProfileFieldOriginCustomNumeric = (
    field: keyof LocalizationCustomFieldOrigin,
    rawValue: string
  ): void => {
    const parsed = Number(rawValue);
    if (!Number.isFinite(parsed)) return;
    persistActiveProfile((profile) => {
      const current = normalizeFieldOriginConfig(profile.fieldOrigin);
      const currentCustom = current.custom ?? { x: 0, z: 0, yawDeg: 0 };
      if (currentCustom[field] === parsed && current.mode === 'custom') return profile;
      return {
        ...profile,
        fieldOrigin: {
          mode: 'custom',
          custom: { ...currentCustom, [field]: parsed }
        }
      };
    });
  };

  const setSnapZToGround = (enabled: boolean): void => {
    persistActiveProfile((profile) =>
      Boolean(profile.snapZToGround) === enabled ? profile : { ...profile, snapZToGround: enabled }
    );
  };

  const setSnapRollToGround = (enabled: boolean): void => {
    persistActiveProfile((profile) =>
      Boolean(profile.snapRollToGround) === enabled ? profile : { ...profile, snapRollToGround: enabled }
    );
  };

  const setSnapPitchToGround = (enabled: boolean): void => {
    persistActiveProfile((profile) =>
      Boolean(profile.snapPitchToGround) === enabled ? profile : { ...profile, snapPitchToGround: enabled }
    );
  };

  const setProfileTemporalEnabled = (enabled: boolean): void => {
    persistActiveProfile((profile) => {
      const current = normalizeTemporalSettings(profile.temporalStabilization);
      if (current.enabled === enabled) return profile;
      return {
        ...profile,
        temporalStabilization: { ...current, enabled }
      };
    });
  };

  const setProfileTemporalNumeric = (field: string, rawValue: string): void => {
    if (!isTemporalNumericField(field)) return;
    const nextValue = parseTemporalNumericValue(field, rawValue);
    if (nextValue == null) return;
    persistActiveProfile((profile) => {
      const current = normalizeTemporalSettings(profile.temporalStabilization);
      if (current[field] === nextValue) return profile;
      return {
        ...profile,
        temporalStabilization: { ...current, [field]: nextValue }
      };
    });
  };

  const setSolverTemporalOverrideEnabled = (enabled: boolean): void => {
    const profile = deps.getActiveProfile();
    const solver = deps.getActiveSolver();
    if (!profile || !solver) return;

    const nextSolvers = profile.solvers.map((entry) => {
      if (entry.id !== solver.id) return entry;
      if (enabled) {
        const base = normalizeTemporalSettings(
          entry.temporalStabilization ?? profile.temporalStabilization
        );
        return { ...entry, temporalStabilization: base };
      }
      return { ...entry, temporalStabilization: null };
    });
    void deps.persistProfileUpdate({ ...profile, solvers: nextSolvers });
  };

  const setSolverTemporalEnabled = (enabled: boolean): void => {
    const profile = deps.getActiveProfile();
    const solver = deps.getActiveSolver();
    if (!profile || !solver || !solver.temporalStabilization) return;
    const current = normalizeTemporalSettings(solver.temporalStabilization);
    if (current.enabled === enabled) return;
    const nextSolvers = profile.solvers.map((entry) =>
      entry.id === solver.id
        ? { ...entry, temporalStabilization: { ...current, enabled } }
        : entry
    );
    void deps.persistProfileUpdate({ ...profile, solvers: nextSolvers });
  };

  const setSolverTemporalNumeric = (field: string, rawValue: string): void => {
    if (!isTemporalNumericField(field)) return;
    const profile = deps.getActiveProfile();
    const solver = deps.getActiveSolver();
    if (!profile || !solver || !solver.temporalStabilization) return;
    const nextValue = parseTemporalNumericValue(field, rawValue);
    if (nextValue == null) return;
    const current = normalizeTemporalSettings(solver.temporalStabilization);
    if (current[field] === nextValue) return;
    const nextSolvers = profile.solvers.map((entry) =>
      entry.id === solver.id
        ? { ...entry, temporalStabilization: { ...current, [field]: nextValue } }
        : entry
    );
    void deps.persistProfileUpdate({ ...profile, solvers: nextSolvers });
  };

  const setSolverRuntimeTuningNumeric = (field: string, rawValue: string): void => {
    if (!isSolverRuntimeNumericField(field)) return;
    const profile = deps.getActiveProfile();
    const solver = deps.getActiveSolver();
    if (!profile || !solver) return;
    const nextValue = parseSolverRuntimeNumericValue(field, rawValue);
    if (nextValue == null) return;
    const current = normalizeSolverRuntimeTuning(solver.runtimeTuning);
    const nextRuntime = normalizeSolverRuntimeTuning({ ...current, [field]: nextValue });
    if (current[field] === nextRuntime[field]) return;
    const nextSolvers = profile.solvers.map((entry) =>
      entry.id === solver.id ? { ...entry, runtimeTuning: nextRuntime } : entry
    );
    void deps.persistProfileUpdate({ ...profile, solvers: nextSolvers });
  };

  return {
    setProfileFieldOriginMode,
    setProfileFieldOriginCustomNumeric,
    setSnapZToGround,
    setSnapRollToGround,
    setSnapPitchToGround,
    setProfileTemporalEnabled,
    setProfileTemporalNumeric,
    setSolverTemporalOverrideEnabled,
    setSolverTemporalEnabled,
    setSolverTemporalNumeric,
    setSolverRuntimeTuningNumeric
  };
};
