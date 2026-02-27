import { writable, type Writable } from 'svelte/store';
import { getLocalStorage, readJson, writeJson } from '$lib/utils/storage';
import type { PipelineDataType, PipelineTypeDescriptor } from '$lib/types/pipeline';

export type FloatingPipelineOutputsViewerState = {
  isOpen: boolean;
  streamId: string | null;
  streamLabel: string | null;
  portTypesByName: Record<string, PipelineDataType | null | undefined>;
  typePalette: Record<string, PipelineTypeDescriptor>;
  position: { x: number; y: number };
  size: { width: number; height: number };
};

const DEFAULT_STATE: FloatingPipelineOutputsViewerState = {
  isOpen: false,
  streamId: null,
  streamLabel: null,
  portTypesByName: {},
  typePalette: {},
  position: { x: 60, y: 60 },
  size: { width: 820, height: 560 }
};

const STORAGE_KEY = 'helios.floatingPipelineOutputsViewer.v1';

const cloneState = (state: FloatingPipelineOutputsViewerState): FloatingPipelineOutputsViewerState => ({
  isOpen: state.isOpen,
  streamId: state.streamId,
  streamLabel: state.streamLabel,
  portTypesByName: { ...(state.portTypesByName ?? {}) },
  typePalette: { ...(state.typePalette ?? {}) },
  position: { ...state.position },
  size: { ...state.size }
});

const loadStoredState = (): Partial<FloatingPipelineOutputsViewerState> | null => {
  const parsed = readJson<Record<string, unknown>>(STORAGE_KEY, null);
  if (!parsed || typeof parsed !== 'object') return null;
  const record = parsed as Record<string, unknown>;
  const position = record.position && typeof record.position === 'object' ? (record.position as Record<string, unknown>) : null;
  const size = record.size && typeof record.size === 'object' ? (record.size as Record<string, unknown>) : null;
  return {
    isOpen: Boolean(record.isOpen),
    position: position
      ? { x: Number(position.x ?? DEFAULT_STATE.position.x), y: Number(position.y ?? DEFAULT_STATE.position.y) }
      : undefined,
    size: size
      ? { width: Number(size.width ?? DEFAULT_STATE.size.width), height: Number(size.height ?? DEFAULT_STATE.size.height) }
      : undefined
  };
};

const saveStoredState = (state: FloatingPipelineOutputsViewerState): void => {
  writeJson(STORAGE_KEY, {
    isOpen: state.isOpen,
    position: state.position,
    size: state.size
  });
};

export interface FloatingPipelineOutputsViewerStore extends Writable<FloatingPipelineOutputsViewerState> {
  open: (params: {
    streamId: string;
    streamLabel?: string | null;
    portTypesByName?: Record<string, PipelineDataType | null | undefined>;
    typePalette?: Record<string, PipelineTypeDescriptor>;
    position?: Partial<FloatingPipelineOutputsViewerState['position']>;
    size?: Partial<FloatingPipelineOutputsViewerState['size']>;
  }) => void;
  close: () => void;
  toggle: (params?: { streamId?: string | null; streamLabel?: string | null }) => void;
  setPosition: (position: FloatingPipelineOutputsViewerState['position']) => void;
  setSize: (size: FloatingPipelineOutputsViewerState['size']) => void;
  getDefaultState: () => FloatingPipelineOutputsViewerState;
}

export function createFloatingPipelineOutputsViewerStore(
  initial: FloatingPipelineOutputsViewerState = DEFAULT_STATE
): FloatingPipelineOutputsViewerStore {
  const store = writable<FloatingPipelineOutputsViewerState>(cloneState(initial));

  return {
    subscribe: store.subscribe,
    set: store.set,
    update: store.update,
    open(params) {
      store.update((state) => ({
        ...state,
        isOpen: true,
        streamId: params.streamId,
        streamLabel: params.streamLabel ?? state.streamLabel,
        portTypesByName: params.portTypesByName ? { ...params.portTypesByName } : state.portTypesByName,
        typePalette: params.typePalette ? { ...params.typePalette } : state.typePalette,
        position: params.position ? { ...state.position, ...params.position } : state.position,
        size: params.size ? { ...state.size, ...params.size } : state.size
      }));
    },
    close() {
      store.update((state) => ({ ...state, isOpen: false }));
    },
    toggle(params) {
      store.update((state) => {
        if (!state.isOpen) {
          return {
            ...state,
            isOpen: true,
            streamId: params?.streamId ?? state.streamId,
            streamLabel: params?.streamLabel ?? state.streamLabel
          };
        }
        return { ...state, isOpen: false };
      });
    },
    setPosition(position) {
      store.update((state) => ({ ...state, position: { ...position } }));
    },
    setSize(size) {
      store.update((state) => ({ ...state, size: { ...size } }));
    },
    getDefaultState() {
      return cloneState(initial);
    }
  };
}

const stored = loadStoredState();
const bootState: FloatingPipelineOutputsViewerState = stored
  ? {
      ...cloneState(DEFAULT_STATE),
      ...stored,
      position: stored.position ? { ...DEFAULT_STATE.position, ...stored.position } : { ...DEFAULT_STATE.position },
      size: stored.size ? { ...DEFAULT_STATE.size, ...stored.size } : { ...DEFAULT_STATE.size }
    }
  : cloneState(DEFAULT_STATE);

export const floatingPipelineOutputsViewer = createFloatingPipelineOutputsViewerStore(bootState);

if (getLocalStorage()) {
  let pending: ReturnType<typeof setTimeout> | null = null;
  floatingPipelineOutputsViewer.subscribe((state) => {
    if (pending) clearTimeout(pending);
    pending = setTimeout(() => {
      pending = null;
      saveStoredState(state);
    }, 120);
  });
}

