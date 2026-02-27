import type { LightingSettings } from '../../../../routes/settings/types';
import { buildWsUrlFromHttpBase, canUseWebSockets } from '$lib/api/wsClient';
import type {
  LightingAnimationListResponse,
  LightingAnimationPayload,
  LightingAnimationTemplateDocument,
  LightingAnimationTemplateSummary,
  LightingColorPayload,
  LightingRuntimeState,
  SavedLightingAnimation
} from './lightingModalUtils';

export async function fetchSavedAnimations(apiFetch: <T>(path: string, init?: RequestInit) => Promise<T>): Promise<SavedLightingAnimation[]> {
  const payload = await apiFetch<LightingAnimationListResponse>('/device/lighting/animations');
  return Array.isArray(payload?.animations) ? payload.animations : [];
}

export async function fetchLightingTemplates(
  apiFetch: <T>(path: string, init?: RequestInit) => Promise<T>
): Promise<LightingAnimationTemplateSummary[]> {
  const payload = await apiFetch<LightingAnimationTemplateSummary[]>('/device/lighting/templates');
  return Array.isArray(payload) ? payload : [];
}

export async function fetchLightingTemplate(
  apiFetch: <T>(path: string, init?: RequestInit) => Promise<T>,
  templateId: string
): Promise<LightingAnimationTemplateDocument> {
  return apiFetch<LightingAnimationTemplateDocument>(`/device/lighting/templates/${encodeURIComponent(templateId)}`);
}

export async function fetchLightingRuntimeState(
  apiFetch: <T>(path: string, init?: RequestInit) => Promise<T>
): Promise<LightingRuntimeState> {
  return apiFetch<LightingRuntimeState>('/device/lighting/state');
}

export async function saveLightingConfig(
  deviceSettingsStore: { patch: (payload: any) => Promise<void> },
  payload: any
): Promise<void> {
  await deviceSettingsStore.patch(payload);
}

export async function resetLightingConfig(apiFetch: <T>(path: string, init?: RequestInit) => Promise<T>): Promise<LightingSettings> {
  return apiFetch<LightingSettings>('/device/lighting/config/reset', { method: 'POST' });
}

export async function postLightingFrame(
  apiFetch: <T>(path: string, init?: RequestInit) => Promise<T>,
  frame: LightingColorPayload[],
  brightness: number,
  requestedBy: string
): Promise<void> {
  const body: Record<string, unknown> = { frame, brightness, requested_by: requestedBy };
  await apiFetch('/device/lighting', { method: 'POST', body: JSON.stringify(body) });
}

export async function postLightingAnimation(
  apiFetch: <T>(path: string, init?: RequestInit) => Promise<T>,
  animation: LightingAnimationPayload,
  brightness: number,
  requestedBy: string
): Promise<void> {
  const body: Record<string, unknown> = { animation, brightness, requested_by: requestedBy };
  await apiFetch('/device/lighting', { method: 'POST', body: JSON.stringify(body) });
}

export async function saveLightingAnimation(
  apiFetch: <T>(path: string, init?: RequestInit) => Promise<T>,
  body: Record<string, unknown>
): Promise<void> {
  await apiFetch('/device/lighting/animations', { method: 'POST', body: JSON.stringify(body) });
}

export async function deleteLightingAnimation(
  apiFetch: <T>(path: string, init?: RequestInit) => Promise<T>,
  name: string
): Promise<void> {
  await apiFetch(`/device/lighting/animations/${encodeURIComponent(name)}`, { method: 'DELETE' });
}

export async function playSavedLightingAnimation(
  apiFetch: <T>(path: string, init?: RequestInit) => Promise<T>,
  body: Record<string, unknown>
): Promise<void> {
  await apiFetch('/device/lighting', { method: 'POST', body: JSON.stringify(body) });
}

export async function stopLightingOutput(
  apiFetch: <T>(path: string, init?: RequestInit) => Promise<T>,
  frame: LightingColorPayload[],
  requestedBy: string
): Promise<void> {
  const body: Record<string, unknown> = { frame, brightness: 0, requested_by: requestedBy };
  await apiFetch('/device/lighting', { method: 'POST', body: JSON.stringify(body) });
}

export function openLightingStateSocket(handlers: {
  onState: (state: LightingRuntimeState) => void;
  onOpen?: () => void;
  onClose?: () => void;
  onError?: (message: string) => void;
}): () => void {
  if (!canUseWebSockets()) {
    handlers.onError?.('WebSocket not supported in this environment.');
    return () => {};
  }

  const url = buildWsUrlFromHttpBase(['v1', 'ws', 'sensors']);
  const socket = new WebSocket(url);
  let closed = false;

  socket.onopen = () => {
    handlers.onOpen?.();
    socket.send(
      JSON.stringify({
        op: 'subscribe',
        kinds: ['lighting'],
        interval_ms: 200
      })
    );
  };
  socket.onmessage = (event) => {
    try {
      const parsed = JSON.parse(String(event.data ?? 'null')) as { lighting?: LightingRuntimeState };
      if (parsed && typeof parsed === 'object' && parsed.lighting && typeof parsed.lighting === 'object') {
        handlers.onState(parsed.lighting);
      }
    } catch {
      // Ignore malformed payloads from unrelated messages.
    }
  };
  socket.onerror = () => {
    if (!closed) {
      handlers.onError?.('Lighting state socket error');
    }
  };
  socket.onclose = () => {
    closed = true;
    handlers.onClose?.();
  };

  return () => {
    if (closed) return;
    closed = true;
    socket.close();
  };
}

type MediaItem = {
  name: string;
  content_type?: string | null;
  tags?: string[] | null;
  kind?: string | null;
  size_bytes?: number;
};

export type LightingMediaPackItem = {
  name: string;
  sizeBytes: number;
  contentType: string;
  tags: string[];
};

export async function listLightingMediaPacks(apiFetch: <T>(path: string, init?: RequestInit) => Promise<T>): Promise<LightingMediaPackItem[]> {
  const payload = await apiFetch<MediaItem[]>('/media?kind=data');
  if (!Array.isArray(payload)) return [];
  return payload
    .filter((item) => {
      const name = item.name ?? '';
      const tags = Array.isArray(item.tags) ? item.tags : [];
      const hasLightingTag = tags.some((tag) => ['lighting', 'lighting-animation-pack', 'lighting-animations'].includes(tag.toLowerCase()));
      const looksLikePackName = /\.json$/i.test(name) && /lighting|animation/i.test(name);
      return hasLightingTag || looksLikePackName;
    })
    .map((item) => ({
      name: item.name,
      sizeBytes: Number(item.size_bytes) || 0,
      contentType: (item.content_type ?? '').trim() || 'application/octet-stream',
      tags: Array.isArray(item.tags) ? item.tags : []
    }))
    .sort((a, b) => a.name.localeCompare(b.name));
}

export async function uploadLightingMediaPack(
  apiFetch: <T>(path: string, init?: RequestInit) => Promise<T>,
  fileName: string,
  jsonPayload: string
): Promise<void> {
  const form = new FormData();
  form.set('file', new Blob([jsonPayload], { type: 'application/json' }), fileName);
  form.set('kind', 'data');
  form.set('description', 'Lighting animation package');
  form.set('tags', 'lighting,lighting-animation-pack');
  await apiFetch('/media', { method: 'POST', body: form });
}

export async function fetchLightingMediaPack(
  apiFetch: <T>(path: string, init?: RequestInit) => Promise<T>,
  name: string
): Promise<unknown> {
  return apiFetch<unknown>(`/media/${encodeURIComponent(name)}`);
}
