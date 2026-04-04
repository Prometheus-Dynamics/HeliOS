import type { StreamCrosshair, StreamCrop, StreamOrderingMode } from './cameraPageContextUtils';

type OverlayState = {
  stream?: { id?: string | null } | null;
  streamCrop: StreamCrop;
  streamCropApplying: boolean;
  streamCropError: string | null;
  streamCropWarning: string | null;
  streamCrosshair: StreamCrosshair;
  streamCrosshairEnabled: boolean;
  streamCrosshairApplying: boolean;
  streamCrosshairError: string | null;
  streamCrosshairWarning: string | null;
  streamOrderingMode: string;
  streamOrderingApplying: boolean;
  streamOrderingError: string | null;
  streamOrderingWarning: string | null;
};

function normalizeWarningList(raw: unknown): string[] {
  if (!Array.isArray(raw)) return [];
  return raw.map((value) => String(value).trim()).filter((value) => value.length > 0);
}

export function createCameraStreamOverlayRuntime({
  streamState,
  getStreamId,
  apiPath,
  apiFetchResponse,
  normalizeStreamCrop,
  normalizeStreamCrosshair,
  normalizeStreamOrderingMode
}: {
  streamState: OverlayState;
  getStreamId: () => string;
  apiPath: (path: string) => string;
  apiFetchResponse: (pathOrUrl: string, init?: RequestInit) => Promise<Response>;
  normalizeStreamCrop: (crop: StreamCrop) => StreamCrop;
  normalizeStreamCrosshair: (crosshair: StreamCrosshair) => StreamCrosshair;
  normalizeStreamOrderingMode: (mode: unknown) => StreamOrderingMode;
}) {
  async function applyStreamCrop(crop: StreamCrop): Promise<void> {
    const effectiveId = streamState.stream?.id ?? getStreamId();
    if (!effectiveId) return;

    const normalized = normalizeStreamCrop(crop);
    streamState.streamCrop = normalized;
    streamState.streamCropError = null;
    streamState.streamCropWarning = null;
    streamState.streamCropApplying = true;

    try {
      const response = await apiFetchResponse(apiPath(`/streams/${encodeURIComponent(effectiveId)}/crop`), {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({ crop: normalized })
      });
      if (!response.ok) {
        const text = await response.text().catch(() => '');
        throw new Error(text || `HTTP ${response.status}`);
      }
      const payload = (await response.json().catch(() => null)) as {
        crop?: number[];
        warnings?: string[];
        roi_inputs_used?: boolean;
      } | null;
      if (Array.isArray(payload?.crop) && payload.crop.length === 4) {
        streamState.streamCrop = normalizeStreamCrop([
          Number(payload.crop[0] ?? normalized[0]),
          Number(payload.crop[1] ?? normalized[1]),
          Number(payload.crop[2] ?? normalized[2]),
          Number(payload.crop[3] ?? normalized[3])
        ]);
      }
      const warnings = normalizeWarningList(payload?.warnings);
      if (warnings.length > 0) {
        streamState.streamCropWarning = warnings[0];
      } else if (payload?.roi_inputs_used === false) {
        streamState.streamCropWarning = 'ROI inputs are not consumed by the active graph.';
      } else {
        streamState.streamCropWarning = null;
      }
    } catch (error) {
      streamState.streamCropError = error instanceof Error ? error.message : 'Crop update failed.';
      streamState.streamCropWarning = null;
      console.error('Failed to apply stream crop', error);
    } finally {
      streamState.streamCropApplying = false;
    }
  }

  async function refreshStreamInputUsageWarnings(): Promise<void> {
    const effectiveId = streamState.stream?.id ?? getStreamId();
    if (!effectiveId) return;
    try {
      const response = await apiFetchResponse(apiPath(`/streams/${encodeURIComponent(effectiveId)}/pipeline/input-usage`), {
        method: 'GET',
        headers: { accept: 'application/json' }
      });
      if (!response.ok) {
        if (response.status === 404) return;
        const text = await response.text().catch(() => '');
        throw new Error(text || `HTTP ${response.status}`);
      }
      const payload = (await response.json().catch(() => null)) as {
        roi_warnings?: string[];
        crosshair_warnings?: string[];
        ordering_warnings?: string[];
        roi_inputs_used?: boolean;
        crosshair_inputs_used?: boolean;
        ordering_inputs_used?: boolean;
      } | null;

      const roiWarnings = normalizeWarningList(payload?.roi_warnings);
      if (roiWarnings.length > 0) {
        streamState.streamCropWarning = roiWarnings[0];
      } else if (payload?.roi_inputs_used === false) {
        streamState.streamCropWarning = 'ROI inputs are not consumed by the active graph.';
      } else {
        streamState.streamCropWarning = null;
      }

      const crosshairWarnings = normalizeWarningList(payload?.crosshair_warnings);
      if (crosshairWarnings.length > 0) {
        streamState.streamCrosshairWarning = crosshairWarnings[0];
      } else if (payload?.crosshair_inputs_used === false) {
        streamState.streamCrosshairWarning = 'Crosshair inputs are not consumed by the active graph.';
      } else {
        streamState.streamCrosshairWarning = null;
      }

      const orderingWarnings = normalizeWarningList(payload?.ordering_warnings);
      if (orderingWarnings.length > 0) {
        streamState.streamOrderingWarning = orderingWarnings[0];
      } else if (payload?.ordering_inputs_used === false) {
        streamState.streamOrderingWarning = 'Ordering input is not consumed by the active graph.';
      } else {
        streamState.streamOrderingWarning = null;
      }
    } catch (error) {
      console.warn('Failed to refresh stream input usage warnings', error);
    }
  }

  async function applyStreamCrosshair(
    crosshair: StreamCrosshair,
    enabled: boolean = streamState.streamCrosshairEnabled
  ): Promise<void> {
    const effectiveId = streamState.stream?.id ?? getStreamId();
    if (!effectiveId) return;

    const normalized = normalizeStreamCrosshair(crosshair);
    const normalizedEnabled = Boolean(enabled);
    streamState.streamCrosshair = normalized;
    streamState.streamCrosshairEnabled = normalizedEnabled;
    streamState.streamCrosshairError = null;
    streamState.streamCrosshairWarning = null;
    streamState.streamCrosshairApplying = true;

    try {
      const response = await apiFetchResponse(apiPath(`/streams/${encodeURIComponent(effectiveId)}/crosshair`), {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({ crosshair: normalized, enabled: normalizedEnabled })
      });
      if (!response.ok) {
        const text = await response.text().catch(() => '');
        throw new Error(text || `HTTP ${response.status}`);
      }
      const payload = (await response.json().catch(() => null)) as {
        crosshair?: number[];
        enabled?: boolean;
        warnings?: string[];
        crosshair_inputs_used?: boolean;
      } | null;
      if (Array.isArray(payload?.crosshair) && payload.crosshair.length === 2) {
        streamState.streamCrosshair = normalizeStreamCrosshair([
          Number(payload.crosshair[0] ?? normalized[0]),
          Number(payload.crosshair[1] ?? normalized[1])
        ]);
      }
      if (typeof payload?.enabled === 'boolean') {
        streamState.streamCrosshairEnabled = payload.enabled;
      }
      const warnings = normalizeWarningList(payload?.warnings);
      if (warnings.length > 0) {
        streamState.streamCrosshairWarning = warnings[0];
      } else if (payload?.crosshair_inputs_used === false) {
        streamState.streamCrosshairWarning = 'Crosshair inputs are not consumed by the active graph.';
      } else {
        streamState.streamCrosshairWarning = null;
      }
    } catch (error) {
      streamState.streamCrosshairError = error instanceof Error ? error.message : 'Crosshair update failed.';
      streamState.streamCrosshairWarning = null;
      console.error('Failed to apply stream crosshair', error);
    } finally {
      streamState.streamCrosshairApplying = false;
    }
  }

  async function applyStreamOrdering(mode: StreamOrderingMode): Promise<void> {
    const effectiveId = streamState.stream?.id ?? getStreamId();
    if (!effectiveId) return;

    const normalized = normalizeStreamOrderingMode(mode);
    streamState.streamOrderingMode = normalized;
    streamState.streamOrderingError = null;
    streamState.streamOrderingWarning = null;
    streamState.streamOrderingApplying = true;

    try {
      const response = await apiFetchResponse(apiPath(`/streams/${encodeURIComponent(effectiveId)}/ordering`), {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({ mode: normalized })
      });
      if (!response.ok) {
        const text = await response.text().catch(() => '');
        throw new Error(text || `HTTP ${response.status}`);
      }
      const payload = (await response.json().catch(() => null)) as {
        mode?: string;
        warnings?: string[];
        ordering_inputs_used?: boolean;
      } | null;
      if (typeof payload?.mode === 'string') {
        streamState.streamOrderingMode = normalizeStreamOrderingMode(payload.mode);
      }
      const warnings = normalizeWarningList(payload?.warnings);
      if (warnings.length > 0) {
        streamState.streamOrderingWarning = warnings[0];
      } else if (payload?.ordering_inputs_used === false) {
        streamState.streamOrderingWarning = 'Ordering input is not consumed by the active graph.';
      } else {
        streamState.streamOrderingWarning = null;
      }
    } catch (error) {
      streamState.streamOrderingError = error instanceof Error ? error.message : 'Ordering update failed.';
      streamState.streamOrderingWarning = null;
      console.error('Failed to apply stream ordering', error);
    } finally {
      streamState.streamOrderingApplying = false;
    }
  }

  return {
    applyStreamCrop,
    refreshStreamInputUsageWarnings,
    applyStreamCrosshair,
    applyStreamOrdering
  };
}
