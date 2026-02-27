import { writable } from 'svelte/store';

export type AsyncState = {
  busy: boolean;
  error: string | null;
  status: string | null;
};

export function createAsyncState(initial: Partial<AsyncState> = {}) {
  const state = writable<AsyncState>({
    busy: false,
    error: null,
    status: null,
    ...initial
  });

  const setBusy = (busy: boolean) => {
    state.update((current) => ({ ...current, busy }));
  };

  const setError = (error: string | null) => {
    state.update((current) => ({ ...current, error }));
  };

  const setStatus = (status: string | null) => {
    state.update((current) => ({ ...current, status }));
  };

  const reset = () => {
    state.set({ busy: false, error: null, status: null });
  };

  return { state, setBusy, setError, setStatus, reset };
}
