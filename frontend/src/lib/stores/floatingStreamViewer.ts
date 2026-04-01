import { writable, type Writable } from 'svelte/store';
import type { StreamPreviewFormat } from '$lib/api/streamPreviewFormat';
import { getLocalStorage, readJson, writeJson } from '$lib/utils/storage';

export type FloatingStreamStatus = 'live' | 'degraded' | 'offline' | 'idle';
export type FloatingStreamFormat = 'auto' | 'mjpeg' | 'h264' | 'h265';

export type FloatingStreamSource = {
  name: string;
  status: FloatingStreamStatus;
  captureSessionId: string | null;
  captureSessionAlias: string | null;
  cameraUid: string | null;
  recordingActive?: boolean;
  recordingSinceMs?: number | null;
  pipelineId?: string | null;
  pipelineOutput?: string | null;
  previewFormat?: FloatingStreamFormat;
  previewFormatHint?: StreamPreviewFormat | null;
};

export type FloatingStreamViewerState = {
  isOpen: boolean;
  stream: FloatingStreamSource | null;
  position: { x: number; y: number };
  size: { width: number; height: number };
};

const DEFAULT_STATE: FloatingStreamViewerState = {
  isOpen: false,
  stream: null,
  position: { x: 40, y: 40 },
  size: { width: 420, height: 236 }
};

const STORAGE_KEY = 'helios.floatingStreamViewer.v1';

const loadStoredState = (): Partial<FloatingStreamViewerState> | null => {
  const parsed = readJson<Record<string, unknown>>(STORAGE_KEY, null);
  if (!parsed || typeof parsed !== 'object') return null;
  const record = parsed as Record<string, unknown>;
  const position = record.position && typeof record.position === 'object' ? (record.position as Record<string, unknown>) : null;
  const size = record.size && typeof record.size === 'object' ? (record.size as Record<string, unknown>) : null;
  const stream = record.stream && typeof record.stream === 'object' ? (record.stream as Record<string, unknown>) : null;
  return {
    isOpen: Boolean(record.isOpen),
    position: position
      ? { x: Number(position.x ?? DEFAULT_STATE.position.x), y: Number(position.y ?? DEFAULT_STATE.position.y) }
      : undefined,
    size: size
      ? { width: Number(size.width ?? DEFAULT_STATE.size.width), height: Number(size.height ?? DEFAULT_STATE.size.height) }
      : undefined,
    stream: stream ? (stream as unknown as FloatingStreamSource) : null
  };
};

const saveStoredState = (state: FloatingStreamViewerState): void => {
  writeJson(STORAGE_KEY, {
    isOpen: state.isOpen,
    position: state.position,
    size: state.size,
    stream: state.stream
  });
};

type PartialStateOptions = {
  position?: Partial<FloatingStreamViewerState['position']>;
  size?: Partial<FloatingStreamViewerState['size']>;
};

const mergeStateOptions = (
  state: FloatingStreamViewerState,
  options?: PartialStateOptions
): FloatingStreamViewerState => {
  if (!options) return state;
  const next = { ...state };
  if (options.position) {
    next.position = { ...state.position, ...options.position };
  }
  if (options.size) {
    next.size = { ...state.size, ...options.size };
  }
  return next;
};

const cloneState = (state: FloatingStreamViewerState): FloatingStreamViewerState => ({
  isOpen: state.isOpen,
  stream: state.stream ? { ...state.stream } : null,
  position: { ...state.position },
  size: { ...state.size }
});

export interface FloatingStreamViewerStore extends Writable<FloatingStreamViewerState> {
  open: (
    stream?: FloatingStreamSource | null,
    options?: PartialStateOptions
  ) => void;
  close: () => void;
  toggle: (
    stream?: FloatingStreamSource | null,
    options?: PartialStateOptions
  ) => void;
  setStream: (stream: FloatingStreamSource | null) => void;
  setPosition: (position: FloatingStreamViewerState['position']) => void;
  setSize: (size: FloatingStreamViewerState['size']) => void;
  reset: () => void;
  getDefaultState: () => FloatingStreamViewerState;
}

export function createFloatingStreamViewerStore(
  initial: FloatingStreamViewerState = DEFAULT_STATE
): FloatingStreamViewerStore {
  const store = writable<FloatingStreamViewerState>(cloneState(initial));

  return {
    subscribe: store.subscribe,
    set: store.set,
    update: store.update,
    open(stream, options) {
      store.update((state) => {
        const next = mergeStateOptions(state, options);
        return {
          ...next,
          isOpen: true,
          stream: stream ? { ...stream } : next.stream
        };
      });
    },
    close() {
      store.update((state) => ({ ...state, isOpen: false }));
    },
    toggle(stream, options) {
      store.update((state) => {
        if (!state.isOpen) {
          const next = mergeStateOptions(state, options);
          return {
            ...next,
            isOpen: true,
            stream: stream ? { ...stream } : next.stream
          };
        }
        return { ...state, isOpen: false };
      });
    },
    setStream(stream) {
      store.update((state) => ({
        ...state,
        stream: stream ? { ...stream } : null
      }));
    },
    setPosition(position) {
      store.update((state) => ({ ...state, position: { ...position } }));
    },
    setSize(size) {
      store.update((state) => ({ ...state, size: { ...size } }));
    },
    reset() {
      store.set(cloneState(initial));
    },
    getDefaultState() {
      return cloneState(initial);
    }
  };
}

const stored = loadStoredState();
const bootState: FloatingStreamViewerState = stored
  ? {
      ...cloneState(DEFAULT_STATE),
      ...stored,
      position: stored.position ? { ...DEFAULT_STATE.position, ...stored.position } : { ...DEFAULT_STATE.position },
      size: stored.size ? { ...DEFAULT_STATE.size, ...stored.size } : { ...DEFAULT_STATE.size }
    }
  : cloneState(DEFAULT_STATE);

export const floatingStreamViewer = createFloatingStreamViewerStore(bootState);

if (getLocalStorage()) {
  let pending: ReturnType<typeof setTimeout> | null = null;
  floatingStreamViewer.subscribe((state) => {
    if (pending) clearTimeout(pending);
    pending = setTimeout(() => {
      pending = null;
      saveStoredState(state);
    }, 120);
  });
}
