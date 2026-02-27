import { buildErrorMessage } from '$lib/ui/errorPolicy';

export function createSettingsLoader(options: {
  load: (options?: { quiet?: boolean; force?: boolean }) => Promise<void>;
  setBusy: (busy: boolean) => void;
  setError: (error: string | null) => void;
  fallback: string;
}) {
  return async function ensureSettings(loadOptions?: { quiet?: boolean; force?: boolean }): Promise<void> {
    options.setBusy(true);
    try {
      await options.load(loadOptions);
      options.setError(null);
    } catch (err) {
      options.setError(buildErrorMessage({ error: err, fallback: options.fallback }));
    } finally {
      options.setBusy(false);
    }
  };
}
