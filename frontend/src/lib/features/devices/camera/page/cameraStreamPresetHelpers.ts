export type CameraStreamPresetDeps = {
  getPipelineUiHydrated: () => boolean;
  scheduleStreamPresetApplyRaw: () => void;
};

export const createCameraStreamPresetHelpers = (deps: CameraStreamPresetDeps) => {
  const scheduleStreamPresetApply = (): void => {
    if (!deps.getPipelineUiHydrated()) return;
    deps.scheduleStreamPresetApplyRaw();
  };

  return { scheduleStreamPresetApply };
};
