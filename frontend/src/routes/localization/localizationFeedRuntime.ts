import type { LocalizationSolveResponse, LocalizationProfile } from '$lib/features/localization/localizationConfig';

export type LocalizationFeedRuntimeDeps = {
  getActiveProfile: () => LocalizationProfile | null;
  getHasAnyFeedSources: () => boolean;
  getActiveHasSources: () => boolean;
  getViewProfilesWithSources: () => LocalizationProfile[];
  fetchLocalizationSolve: (profileId: string, signal: AbortSignal) => Promise<LocalizationSolveResponse>;
  setSolveResponsesByProfile: (next: Record<string, LocalizationSolveResponse>) => void;
  setSolveResponse: (next: LocalizationSolveResponse | null) => void;
  setPollResults: (next: LocalizationSolveResponse['sources'] | []) => void;
  setFeedStatus: (next: 'idle' | 'connecting' | 'live' | 'error') => void;
  setFeedMessage: (next: string | null) => void;
  setLastPollMs: (next: number | null) => void;
};

export const createLocalizationFeedRuntime = (deps: LocalizationFeedRuntimeDeps) => {
  const isSuppressedSourceError = (value: string | null | undefined): boolean => {
    const normalized = String(value ?? '').trim().toLowerCase();
    return normalized === 'missing tag size for detections';
  };

  const runFeedPoll = async (signal: AbortSignal): Promise<void> => {
    const profile = deps.getActiveProfile();
    if (!profile || !deps.getHasAnyFeedSources()) return;
    const startAll = performance.now();

    try {
      const profileIds = Array.from(
        new Set([
          ...(deps.getActiveHasSources() ? [profile.id] : []),
          ...deps.getViewProfilesWithSources().map((entry) => entry.id)
        ])
      );
      const results = await Promise.allSettled(profileIds.map((profileId) => deps.fetchLocalizationSolve(profileId, signal)));
      if (signal.aborted) return;
      const nextResponses: Record<string, LocalizationSolveResponse> = {};
      let activeResult: LocalizationSolveResponse | null = null;
      let activeError: string | null = null;

      results.forEach((result, index) => {
        const id = profileIds[index];
        if (result.status === 'fulfilled') {
          nextResponses[id] = result.value;
          if (id === profile.id) {
            activeResult = result.value;
          }
        } else if (id === profile.id) {
          activeError = result.reason instanceof Error ? result.reason.message : 'Failed to fetch localization outputs';
        }
      });

      deps.setSolveResponsesByProfile(nextResponses);
      deps.setSolveResponse(activeResult);
      deps.setPollResults(activeResult?.sources ?? []);
      if (activeResult) {
        const solverErrors = activeResult.solvers.flatMap((solver) => solver.errors ?? []);
        const sourceError =
          activeResult.sources.find((entry) => entry.error && !isSuppressedSourceError(entry.error))
            ?.error ?? null;
        const errorMessage = solverErrors[0] ?? sourceError ?? null;
        deps.setFeedStatus(errorMessage ? 'error' : 'live');
        deps.setFeedMessage(errorMessage);
      } else if (deps.getActiveHasSources() && activeError) {
        deps.setFeedStatus('error');
        deps.setFeedMessage(activeError);
      } else if (deps.getHasAnyFeedSources()) {
        deps.setFeedStatus('live');
        deps.setFeedMessage(null);
      } else {
        deps.setFeedStatus('idle');
        deps.setFeedMessage(null);
      }
    } catch (error) {
      if (signal.aborted) return;
      const message = error instanceof Error ? error.message : 'Failed to fetch localization outputs';
      deps.setSolveResponse(null);
      deps.setSolveResponsesByProfile({});
      deps.setPollResults([]);
      deps.setFeedStatus('error');
      deps.setFeedMessage(message);
    } finally {
      if (!signal.aborted) {
        deps.setLastPollMs(Math.max(0, performance.now() - startAll));
      }
    }
  };

  return { runFeedPoll };
};
