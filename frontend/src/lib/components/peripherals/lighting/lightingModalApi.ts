import type { LedConfig } from '../../../../routes/settings/types';
import { apiFetch } from '$lib/api/core/http';
import { buildWsUrlFromHttpBase, canUseWebSockets, connectWebSocketWithFallback, sendJson } from '$lib/api/core/ws';
import type {
  LightingAnimationListResponse,
  LightingAnimationPayload,
  LightingAnimationTemplateDocument,
  LightingAnimationTemplateSummary,
  LightingColorPayload,
  LightingRuntimeState,
  SavedLightingAnimation
} from './lightingModalUtils';

export async function fetchSavedAnimations(): Promise<SavedLightingAnimation[]> {
  const payload = await apiFetch<LightingAnimationListResponse>('/device/lighting/animations');
  return Array.isArray(payload?.animations) ? payload.animations : [];
}

export async function fetchLightingTemplates(): Promise<LightingAnimationTemplateSummary[]> {
  const payload = await apiFetch<LightingAnimationTemplateSummary[]>('/device/lighting/templates');
  return Array.isArray(payload) ? payload : [];
}

export async function fetchLightingTemplate(templateId: string): Promise<LightingAnimationTemplateDocument> {
  return apiFetch<LightingAnimationTemplateDocument>(`/device/lighting/templates/${encodeURIComponent(templateId)}`);
}

export async function fetchLightingRuntimeState(): Promise<LightingRuntimeState> {
  return apiFetch<LightingRuntimeState>('/device/lighting/state');
}

export async function saveLightingConfig(
  deviceSettingsStore: { patch: (payload: Record<string, unknown>) => Promise<void> },
  payload: Record<string, unknown>
): Promise<void> {
  await deviceSettingsStore.patch(payload);
}

export async function resetLightingConfig(): Promise<LedConfig> {
  return apiFetch<LedConfig>('/device/lighting/config/reset', { method: 'POST' });
}

export async function postLightingFrame(
  frame: LightingColorPayload[],
  brightness: number,
  requestedBy: string
): Promise<void> {
  const body: Record<string, unknown> = { frame, brightness, requested_by: requestedBy };
  await apiFetch('/device/lighting', { method: 'POST', body });
}

export async function postLightingAnimation(
  animation: LightingAnimationPayload,
  brightness: number,
  requestedBy: string
): Promise<void> {
  const body: Record<string, unknown> = { animation, brightness, requested_by: requestedBy };
  await apiFetch('/device/lighting', { method: 'POST', body });
}

export async function saveLightingAnimation(body: Record<string, unknown>): Promise<void> {
  await apiFetch('/device/lighting/animations', { method: 'POST', body });
}

export async function deleteLightingAnimation(name: string): Promise<void> {
  await apiFetch(`/device/lighting/animations/${encodeURIComponent(name)}`, { method: 'DELETE' });
}

export async function playSavedLightingAnimation(body: Record<string, unknown>): Promise<void> {
  await apiFetch('/device/lighting', { method: 'POST', body });
}

export async function stopLightingOutput(
  frame: LightingColorPayload[],
  requestedBy: string
): Promise<void> {
  const body: Record<string, unknown> = { frame, brightness: 0, requested_by: requestedBy };
  await apiFetch('/device/lighting', { method: 'POST', body });
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
  let closed = false;
  const connection = connectWebSocketWithFallback(
    url,
    {
      onOpen: () => {
        handlers.onOpen?.();
        sendJson(connection, {
          op: 'subscribe',
          kinds: ['lighting'],
          interval_ms: 200
        });
      },
      onMessage: (event) => {
        try {
          const parsed = JSON.parse(String(event.data ?? 'null')) as { lighting?: LightingRuntimeState };
          if (parsed && typeof parsed === 'object' && parsed.lighting && typeof parsed.lighting === 'object') {
            handlers.onState(parsed.lighting);
          }
        } catch {
          // Ignore malformed payloads from unrelated messages.
        }
      },
      onError: (message) => {
        if (!closed) {
          handlers.onError?.(message || 'Lighting state socket error');
        }
      },
      onClose: () => {
        closed = true;
        handlers.onClose?.();
      }
    },
    { errorMessage: 'Lighting state socket error' }
  );

  if (!connection) {
    handlers.onError?.('Unable to open lighting state socket');
    return () => {};
  }

  return () => {
    if (closed) return;
    closed = true;
    connection.close();
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

export async function listLightingMediaPacks(): Promise<LightingMediaPackItem[]> {
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

export async function fetchLightingMediaPack(name: string): Promise<unknown> {
  return apiFetch<unknown>(`/media/${encodeURIComponent(name)}`);
}
