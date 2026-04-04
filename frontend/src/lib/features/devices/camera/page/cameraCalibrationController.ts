import type { StreamInfo } from '$lib/api/client';
import { apiFetchResponse } from '$lib/api/core/http';
import type { CalibrationBoard, CalibrationImage, CalibrationParams, CalibrationResult, IpaStatus } from '$lib/features/devices/camera/cameraCalibrationTypes';
import { loadOwnedStreams } from '$lib/api/streamResources';
import { emitMediaMutation } from '$lib/features/media/mutations';
import { compareMediaRecent } from '$lib/features/media/sort';
import { resolveStreamLabel } from '$lib/utils/streamLabels';
import {
  asCcmMatrix,
  asFiniteNumber,
  asRecord,
  ccmMatrixFromFlat,
  normalizeIpaStatus,
  type CalibrationImportSource
} from './cameraCalibrationSupport';

type CalibrationState = {
  get stream(): StreamInfo | null;
  get streamId(): string;
  get calibrationBoard(): CalibrationBoard;
  get calibrationLensModel(): 'pinhole' | 'fisheye';
  set calibrationLensModel(value: 'pinhole' | 'fisheye');
  get calibrationImages(): CalibrationImage[];
  set calibrationImages(value: CalibrationImage[]);
  get calibrationOwnPhotosOnly(): boolean;
  set calibrationOwnPhotosOnly(value: boolean);
  get calibrationSelected(): Record<string, boolean>;
  set calibrationSelected(value: Record<string, boolean>);
  get calibrationLoading(): boolean;
  set calibrationLoading(value: boolean);
  get calibrationSolving(): boolean;
  set calibrationSolving(value: boolean);
  get calibrationApplying(): boolean;
  set calibrationApplying(value: boolean);
  get calibrationDeleting(): boolean;
  set calibrationDeleting(value: boolean);
  get calibrationSolveError(): string | null;
  set calibrationSolveError(value: string | null);
  get calibrationGuidedMode(): boolean;
  set calibrationGuidedMode(value: boolean);
  get calibrationGuidedBusy(): boolean;
  set calibrationGuidedBusy(value: boolean);
  get calibrationGuidedResetToken(): number;
  set calibrationGuidedResetToken(value: number);
  get calibrationGuidedCaptureToken(): number;
  set calibrationGuidedCaptureToken(value: number);
  get calibrationGuidedAccumulateLive(): boolean;
  set calibrationGuidedAccumulateLive(value: boolean);
  get calibrationIncludeOverlays(): boolean;
  set calibrationIncludeOverlays(value: boolean);
  get calibrationResult(): CalibrationResult | null;
  set calibrationResult(value: CalibrationResult | null);
  get calibrationImportSourcesLoading(): boolean;
  set calibrationImportSourcesLoading(value: boolean);
  get calibrationImporting(): boolean;
  set calibrationImporting(value: boolean);
  get calibrationImportError(): string | null;
  set calibrationImportError(value: string | null);
  get calibrationImportSourceId(): string;
  set calibrationImportSourceId(value: string);
  get calibrationImportSources(): CalibrationImportSource[];
  set calibrationImportSources(value: CalibrationImportSource[]);
  get calibrationPreviewOpen(): boolean;
  set calibrationPreviewOpen(value: boolean);
  get calibrationPreviewItem(): CalibrationImage | null;
  set calibrationPreviewItem(value: CalibrationImage | null);
  get ipaLoading(): boolean;
  set ipaLoading(value: boolean);
  get ipaStatus(): IpaStatus | null;
  set ipaStatus(value: IpaStatus | null);
  get ipaTarget(): string;
  set ipaTarget(value: string);
  get ipaCt(): number;
  set ipaCt(value: number);
  get ipaCcm(): number[][] | null;
  set ipaCcm(value: number[][] | null);
  get ipaAdvanced(): boolean;
  set ipaAdvanced(value: boolean);
  get ipaChartModalOpen(): boolean;
  set ipaChartModalOpen(value: boolean);
  get ipaChartImage(): string;
  set ipaChartImage(value: string);
  get ipaChartCorners(): Array<[number, number]>;
  set ipaChartCorners(value: Array<[number, number]>);
  get ipaChartNaturalSize(): { w: number; h: number };
  set ipaChartNaturalSize(value: { w: number; h: number });
  get ipaChartSolveBusy(): boolean;
  set ipaChartSolveBusy(value: boolean);
  get ipaChartSolveError(): string | null;
  set ipaChartSolveError(value: string | null);
  get ipaChartSolveResult(): { ccm: number[][]; rmsError: number } | null;
  set ipaChartSolveResult(value: { ccm: number[][]; rmsError: number } | null);
};

type CalibrationDeps = {
  apiPath: (path: string) => string;
  toaster: {
    success: (payload: { title: string; description?: string }) => void;
    error: (payload: { title: string; description?: string }) => void;
  };
  reportError: (args: {
    title: string;
    error: unknown;
    fallback: string;
    inline?: (message: string) => void;
  }) => string | void;
  refresh: () => Promise<void>;
  normalizeCalibrationSolveResult: (value: unknown) => CalibrationResult | null;
  normalizeCalibrationParams: (value: unknown) => CalibrationParams | null;
};
export function createCameraCalibrationController(state: CalibrationState, deps: CalibrationDeps) {
  function calibrationPayload(params: CalibrationParams) {
    return {
      fx: params.fx,
      fy: params.fy,
      cx: params.cx,
      cy: params.cy,
      k1: params.k1,
      k2: params.k2,
      p1: params.p1,
      p2: params.p2,
      k3: params.k3,
      undistortIters: params.undistortIters ?? 5,
      lensModel: params.lensModel ?? state.calibrationLensModel
    };
  }

  function extractImportedCalibration(payload: unknown): CalibrationParams | null {
    const root = asRecord(payload);
    const manifest = asRecord(root?.manifest);
    const manifestCamera = asRecord(manifest?.camera);
    const stream = asRecord(root?.stream);
    const streamManifest = asRecord(stream?.manifest);
    const streamManifestCamera = asRecord(streamManifest?.camera);
    const candidates = [
      payload,
      root?.calibration,
      root?.camera,
      root?.params,
      manifest?.calibration,
      manifestCamera?.calibration,
      manifestCamera?.intrinsics,
      manifest?.intrinsics,
      streamManifest?.calibration,
      streamManifestCamera?.calibration,
      streamManifestCamera?.intrinsics,
      streamManifest?.intrinsics
    ];

    for (const candidate of candidates) {
      const normalized = deps.normalizeCalibrationParams(candidate);
      if (normalized) return normalized;
    }

    return null;
  }

  async function saveImportedCalibration(params: CalibrationParams, sourceLabel: string): Promise<void> {
    if (state.calibrationImporting || state.calibrationApplying || state.calibrationSolving) return;
    state.calibrationImporting = true;
    state.calibrationImportError = null;
    try {
      const url = deps.apiPath(`/streams/${encodeURIComponent(state.stream?.id ?? state.streamId)}/calibration/save`);
      const resp = await apiFetchResponse(url, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(calibrationPayload(params))
      });
      if (!resp.ok) {
        const text = await resp.text().catch(() => '');
        throw new Error(text || `Import failed (${resp.status})`);
      }

      if (params.lensModel === 'pinhole' || params.lensModel === 'fisheye') {
        state.calibrationLensModel = params.lensModel;
      }
      state.calibrationImportError = null;
      deps.toaster.success({
        title: 'Calibration imported',
        description: `Saved from ${sourceLabel}`
      });
      await deps.refresh();
      await refreshCalibrationImportSources();
    } catch (err) {
      state.calibrationImportError = deps.reportError({
        title: 'Import failed',
        error: err,
        fallback: 'Unable to import calibration right now.'
      }) as string;
    } finally {
      state.calibrationImporting = false;
    }
  }

  async function refreshCalibrationImportSources(): Promise<void> {
    state.calibrationImportSourcesLoading = true;
    state.calibrationImportError = null;
    try {
      const streams = await loadOwnedStreams({ force: true, preferCached: false });
      const currentId = (state.stream?.id ?? state.streamId).trim();
      const nextSources: CalibrationImportSource[] = [];
      for (const stream of streams ?? []) {
        const id = stream?.id?.trim();
        if (!id || id === currentId) continue;
        const params = deps.normalizeCalibrationParams(stream.manifest?.calibration ?? null);
        if (!params) continue;
        nextSources.push({
          id,
          label: resolveStreamLabel(stream, id),
          calibration: params
        });
      }

      nextSources.sort((a, b) => a.label.localeCompare(b.label, undefined, { sensitivity: 'base' }));
      state.calibrationImportSources = nextSources;
      if (!nextSources.some((item) => item.id === state.calibrationImportSourceId)) {
        state.calibrationImportSourceId = nextSources[0]?.id ?? '';
      }
    } catch (err) {
      console.warn('Failed to load calibration import sources', err);
      state.calibrationImportSources = [];
      state.calibrationImportSourceId = '';
      state.calibrationImportError = 'Unable to load other stream calibrations.';
    } finally {
      state.calibrationImportSourcesLoading = false;
    }
  }

  async function copyCalibrationFromSelectedStream(): Promise<void> {
    if (state.calibrationImporting || state.calibrationApplying || state.calibrationSolving) return;
    const selectedId = state.calibrationImportSourceId?.trim?.() ?? '';
    if (!selectedId) {
      deps.toaster.error({ title: 'Pick a stream', description: 'Select a source stream with saved calibration.' });
      return;
    }

    const source = state.calibrationImportSources.find((item) => item.id === selectedId);
    if (!source) {
      await refreshCalibrationImportSources();
      const refreshed = state.calibrationImportSources.find((item) => item.id === selectedId);
      if (!refreshed) {
        deps.toaster.error({ title: 'Source unavailable', description: 'Refresh stream list and try again.' });
        return;
      }
      await saveImportedCalibration(refreshed.calibration, refreshed.label);
      return;
    }

    await saveImportedCalibration(source.calibration, source.label);
  }

  async function importCalibrationFromJsonFile(file: File): Promise<void> {
    if (!file) return;
    if (state.calibrationImporting || state.calibrationApplying || state.calibrationSolving) return;
    state.calibrationImportError = null;
    try {
      const raw = await file.text();
      const payload = JSON.parse(raw) as unknown;
      const params = extractImportedCalibration(payload);
      if (!params) {
        throw new Error('No valid calibration fields were found in this JSON file.');
      }
      await saveImportedCalibration(params, file.name || 'JSON file');
    } catch (err) {
      state.calibrationImportError = deps.reportError({
        title: 'Import failed',
        error: err,
        fallback: 'Unable to parse calibration JSON.'
      }) as string;
    }
  }

  async function refreshIpaStatus(): Promise<void> {
    state.ipaLoading = true;
    try {
      const resp = await apiFetchResponse(deps.apiPath('/device/ipa'));
      if (!resp.ok) throw new Error(`Failed to load IPA status (${resp.status})`);
      const json = (await resp.json()) as unknown;
      const status = normalizeIpaStatus(json);
      state.ipaStatus = status;
      const first = status?.files.find((file) => Array.isArray(file.ccm) && file.ccm.length === 9) ?? null;
      if (first?.ccm) {
        state.ipaCcm = ccmMatrixFromFlat(first.ccm);
      }
      if (typeof first?.ccmCt === 'number') {
        state.ipaCt = Number(first.ccmCt) || 4000;
      }
    } catch (err) {
      console.warn('Failed to load IPA status', err);
      state.ipaStatus = null;
    } finally {
      state.ipaLoading = false;
    }
  }

  async function applyIpaCcm(): Promise<void> {
    if (state.ipaLoading) return;
    state.ipaLoading = true;
    try {
      const url = deps.apiPath('/device/ipa/ccm');
      const target = state.ipaAdvanced ? state.ipaTarget : 'both';
      const resp = await apiFetchResponse(url, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          target,
          ct: state.ipaCt,
          ccm: state.ipaCcm
        })
      });
      if (!resp.ok) {
        const text = await resp.text().catch(() => '');
        throw new Error(text || `Apply failed (${resp.status})`);
      }
      deps.toaster.success({ title: 'IPA updated', description: 'Engine restarted to reload tuning.' });
      await refreshIpaStatus();
    } catch (err) {
      deps.reportError({
        title: 'IPA update failed',
        error: err,
        fallback: 'Unable to update IPA settings right now.'
      });
    } finally {
      state.ipaLoading = false;
    }
  }

  function openCalibrationPreview(item: CalibrationImage) {
    state.calibrationPreviewItem = item;
    state.calibrationPreviewOpen = true;
  }

  function closeCalibrationPreview() {
    state.calibrationPreviewOpen = false;
    state.calibrationPreviewItem = null;
  }

  function openIpaChartSolverForImage(imageName: string) {
    state.ipaChartImage = imageName;
    state.ipaChartCorners = [];
    state.ipaChartNaturalSize = { w: 1, h: 1 };
    state.ipaChartSolveError = null;
    state.ipaChartSolveResult = null;
    state.ipaChartModalOpen = true;
  }

  function closeIpaChartSolver() {
    state.ipaChartModalOpen = false;
  }

  function addIpaChartCorner(event: MouseEvent) {
    if (!state.ipaChartImage || state.ipaChartCorners.length >= 4) return;
    const host = event.currentTarget as HTMLElement | null;
    const imgEl =
      host instanceof HTMLImageElement ? host : ((host?.querySelector?.('img') as HTMLImageElement | null) ?? null);
    if (!imgEl) return;
    const rect = imgEl.getBoundingClientRect();
    if (!rect.width || !rect.height) return;
    const x = ((event.clientX - rect.left) * imgEl.naturalWidth) / rect.width;
    const y = ((event.clientY - rect.top) * imgEl.naturalHeight) / rect.height;
    if (!Number.isFinite(x) || !Number.isFinite(y)) return;
    state.ipaChartCorners = [...state.ipaChartCorners, [x, y]];
  }

  async function solveIpaChartCcm(): Promise<void> {
    if (state.ipaChartSolveBusy) return;
    if (!state.ipaChartImage) {
      deps.toaster.error({ title: 'Pick an image', description: 'Select a chart photo first.' });
      return;
    }
    if (state.ipaChartCorners.length !== 4) {
      deps.toaster.error({ title: 'Pick 4 corners', description: 'Click the chart corners: TL -> TR -> BR -> BL.' });
      return;
    }
    state.ipaChartSolveBusy = true;
    state.ipaChartSolveError = null;
    try {
      const url = deps.apiPath('/device/ipa/ccm/solve');
      const resp = await apiFetchResponse(url, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          image: state.ipaChartImage,
          corners: state.ipaChartCorners,
          chart: 'colorCheckerClassic24'
        })
      });
      if (!resp.ok) {
        const text = await resp.text().catch(() => '');
        throw new Error(text || `Solve failed (${resp.status})`);
      }
      const json = (await resp.json()) as unknown;
      const payload = asRecord(json);
      const ccm = asCcmMatrix(payload?.ccm);
      if (!ccm) {
        throw new Error('CCM solve returned an invalid matrix.');
      }
      const rmsError = asFiniteNumber(payload?.rmsError ?? payload?.rms_error) ?? 0;
      state.ipaChartSolveResult = { ccm, rmsError };
      state.ipaCcm = ccm;
      deps.toaster.success({
        title: 'Solved CCM',
        description: `RMS error ${rmsError.toFixed(4)}`
      });
    } catch (err) {
      deps.reportError({
        title: 'CCM solve failed',
        error: err,
        fallback: 'Unable to solve the chart right now.',
        inline: (message) => {
          state.ipaChartSolveError = message;
        }
      });
    } finally {
      state.ipaChartSolveBusy = false;
    }
  }

  async function setGuidedCalibrationMode(enabled: boolean): Promise<void> {
    if (state.calibrationGuidedBusy) return;
    if (!state.stream?.id) {
      deps.toaster.error({ title: 'Calibration mode unavailable', description: 'Stream UUID not available yet.' });
      return;
    }
    state.calibrationGuidedBusy = true;
    try {
      if (enabled) {
        // Default to snapshot-only guidance when entering guided mode.
        state.calibrationGuidedAccumulateLive = false;
      }
      const url = deps.apiPath(`/streams/${encodeURIComponent(state.stream.id)}/calibration/mode`);
      const dictionary = (state.calibrationBoard?.dictionary || '4x4_1000').trim();
      const resp = await apiFetchResponse(url, {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({ enabled, dictionary, mode: 'calibration' })
      });
      if (!resp.ok) {
        const text = await resp.text().catch(() => '');
        if (text) {
          try {
            const payload = JSON.parse(text) as { error?: string; message?: string };
            throw new Error(payload.error || payload.message || text);
          } catch {
            throw new Error(text);
          }
        }
        throw new Error(`Calibration mode update failed (${resp.status})`);
      }
      state.calibrationGuidedMode = enabled;
      if (!enabled) state.calibrationGuidedResetToken += 1;
    } catch (err) {
      console.error('Failed to toggle guided calibration mode', err);
      deps.reportError({
        title: 'Calibration mode failed',
        error: err,
        fallback: 'Unable to toggle calibration mode right now.'
      });
    } finally {
      state.calibrationGuidedBusy = false;
    }
  }

  function resetGuidedCalibrationCoverage(): void {
    state.calibrationGuidedResetToken += 1;
  }

  async function refreshCalibrationImages(): Promise<void> {
    state.calibrationLoading = true;
    try {
      const streamId = (state.stream?.id ?? '').trim();
      const query = new URLSearchParams();
      if (state.calibrationOwnPhotosOnly && streamId.length > 0) {
        query.set('stream_id', streamId);
      }
      const suffix = query.toString();
      const url = deps.apiPath(suffix ? `/media?${suffix}` : '/media');
      const resp = await apiFetchResponse(url);
      if (!resp.ok) {
        throw new Error(`Failed to list calibration images (${resp.status})`);
      }
      const list = (await resp.json()) as Array<{ name: string; size_bytes: number; content_type: string; stream_id?: string; kind?: string; captured_at_ms?: number }>;
      const images = Array.isArray(list)
        ? list.filter(
            (item) =>
              typeof item?.name === 'string' &&
              typeof item?.content_type === 'string' &&
              item.content_type.toLowerCase().startsWith('image/')
          )
        : [];
      images.sort(compareMediaRecent);
      state.calibrationImages = images;
      const nextSelected: Record<string, boolean> = {};
      state.calibrationImages.forEach((item) => {
        nextSelected[item.name] = Boolean(state.calibrationSelected[item.name]);
      });
      state.calibrationSelected = nextSelected;
    } catch (err) {
      console.warn('Failed to load calibration images', err);
    } finally {
      state.calibrationLoading = false;
    }
  }

  async function setCalibrationOwnPhotosOnly(enabled: boolean): Promise<void> {
    const next = Boolean(enabled);
    if (state.calibrationOwnPhotosOnly === next) {
      return;
    }
    state.calibrationOwnPhotosOnly = next;
    await refreshCalibrationImages();
  }

  async function takeCalibrationSnapshot(): Promise<void> {
    if (state.calibrationApplying || state.calibrationSolving) return;
    state.calibrationLoading = true;
    try {
      const url = deps.apiPath(`/streams/${encodeURIComponent(state.stream?.id ?? state.streamId)}/snapshot`);
      const resp = await apiFetchResponse(url, {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({
          kind: 'calibration',
          source: { kind: 'raw' }
        })
      });
      if (!resp.ok) {
        const text = await resp.text().catch(() => '');
        throw new Error(text || `Snapshot failed (${resp.status})`);
      }
      await refreshCalibrationImages();
      if (state.calibrationGuidedMode) {
        state.calibrationGuidedCaptureToken += 1;
      }
      emitMediaMutation({
        mutation: 'created',
        mediaKind: 'image',
        cameraSource: state.stream?.id ?? state.streamId ?? null
      });
      deps.toaster.success({ title: 'Snapshot captured', description: 'Saved to media' });
    } catch (err) {
      console.error('Failed to capture snapshot', err);
      deps.reportError({
        title: 'Snapshot failed',
        error: err,
        fallback: 'Unable to capture a snapshot right now.'
      });
    } finally {
      state.calibrationLoading = false;
    }
  }

  async function deleteCalibrationSnapshot(name: string): Promise<void> {
    if (state.calibrationDeleting) return;
    state.calibrationDeleting = true;
    try {
      const url = deps.apiPath(`/media/${encodeURIComponent(name)}`);
      const resp = await apiFetchResponse(url, { method: 'DELETE' });
      if (!resp.ok) {
        const text = await resp.text().catch(() => '');
        throw new Error(text || `Delete failed (${resp.status})`);
      }
      state.calibrationImages = state.calibrationImages.filter((item) => item.name !== name);
      if (state.calibrationSelected[name]) {
        const next = { ...state.calibrationSelected };
        delete next[name];
        state.calibrationSelected = next;
      }
      if (state.ipaChartImage === name) {
        state.ipaChartImage = '';
        if (state.ipaChartModalOpen) closeIpaChartSolver();
      }
      if (state.calibrationPreviewItem?.name === name) closeCalibrationPreview();
      emitMediaMutation({
        mutation: 'deleted',
        mediaKind: 'image',
        cameraSource: state.stream?.id ?? state.streamId ?? null,
        mediaId: name
      });
      deps.toaster.success({ title: 'Snapshot deleted', description: 'Removed from media' });
    } catch (err) {
      console.error('Failed to delete snapshot', err);
      deps.reportError({
        title: 'Delete failed',
        error: err,
        fallback: 'Unable to delete the snapshot right now.'
      });
    } finally {
      state.calibrationDeleting = false;
    }
  }

  async function solveCalibration(): Promise<void> {
    if (state.calibrationSolving || state.calibrationApplying) return;
    const selected = Object.entries(state.calibrationSelected)
      .filter(([, enabled]) => enabled)
      .map(([name]) => name);
    if (!selected.length) {
      deps.toaster.error({ title: 'Select images', description: 'Pick at least one calibration snapshot.' });
      return;
    }

    state.calibrationSolving = true;
    state.calibrationSolveError = null;
    try {
      const markerMm = Math.min(Math.max(0.001, Number(state.calibrationBoard.markerMm)), Number(state.calibrationBoard.squareMm) - 0.001);
      const squareMm = Math.max(markerMm + 0.001, Number(state.calibrationBoard.squareMm));

      const url = deps.apiPath('/calibration/solve');
      const body = {
        images: selected,
        board: {
          squaresX: Math.max(2, Math.trunc(Number(state.calibrationBoard.squaresX))),
          squaresY: Math.max(2, Math.trunc(Number(state.calibrationBoard.squaresY))),
          squareSize: squareMm,
          markerSize: markerMm,
          dictionary: (state.calibrationBoard.dictionary || '4x4_1000').trim()
        },
        includeOverlays: state.calibrationIncludeOverlays,
        config: {
          minViews: 1,
          minPointsPerView: 8,
          refineDistortion: true,
          undistortIters: 5,
          refineUndistortIters: 8,
          lensModel: state.calibrationLensModel
        }
      };
      const resp = await apiFetchResponse(url, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(body)
      });
      if (!resp.ok) {
        const text = await resp.text().catch(() => '');
        throw new Error(text || `Solve failed (${resp.status})`);
      }
      const result = (await resp.json()) as unknown;
      const normalized = deps.normalizeCalibrationSolveResult(result);
      if (!normalized) {
        throw new Error(`Solve returned unexpected payload: ${JSON.stringify(result)}`);
      }
      state.calibrationResult = normalized;
      state.calibrationSolveError = null;
      deps.toaster.success({
        title: 'Calibration solved',
        description: `Reprojection error ~${Number(normalized.reprojectionErrorPx ?? 0).toFixed(2)}px`
      });
    } catch (err) {
      console.error('Calibration solve failed', err);
      state.calibrationSolveError = deps.reportError({
        title: 'Solve failed',
        error: err,
        fallback: 'Unable to solve the calibration right now.'
      }) as string;
    } finally {
      state.calibrationSolving = false;
    }
  }

  async function saveSolvedCalibration(): Promise<void> {
    if (!state.calibrationResult || state.calibrationApplying) return;
    state.calibrationApplying = true;
    try {
      const url = deps.apiPath(`/streams/${encodeURIComponent(state.stream?.id ?? state.streamId)}/calibration/save`);
      const params = state.calibrationResult.calibration;
      const resp = await apiFetchResponse(url, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(calibrationPayload(params))
      });
      if (!resp.ok) {
        const text = await resp.text().catch(() => '');
        throw new Error(text || `Apply failed (${resp.status})`);
      }
      deps.toaster.success({ title: 'Calibration saved', description: 'Calibration stored on the stream manifest.' });
      await deps.refresh();
    } catch (err) {
      console.error('Calibration apply failed', err);
      deps.reportError({
        title: 'Save failed',
        error: err,
        fallback: 'Unable to save the calibration right now.'
      });
    } finally {
      state.calibrationApplying = false;
    }
  }

  async function deleteCalibrationMedia(kind: 'calibration' | 'calibration_overlay'): Promise<void> {
    if (state.calibrationDeleting) return;
    state.calibrationDeleting = true;
    try {
      const streamId = state.stream?.id ?? state.streamId;
      const url = deps.apiPath(`/media?stream_id=${encodeURIComponent(streamId)}&kind=${encodeURIComponent(kind)}`);
      const resp = await apiFetchResponse(url);
      if (!resp.ok) {
        throw new Error(`Failed to list ${kind} media (${resp.status})`);
      }
      const list = (await resp.json()) as Array<{ name: string }>;
      for (const item of list) {
        if (!item?.name) continue;
        await apiFetchResponse(deps.apiPath(`/media/${encodeURIComponent(item.name)}`), { method: 'DELETE' });
      }
      if (kind === 'calibration') {
        await refreshCalibrationImages();
      }
      emitMediaMutation({
        mutation: 'deleted',
        mediaKind: kind === 'calibration' ? 'image' : 'unknown',
        cameraSource: state.stream?.id ?? state.streamId ?? null
      });
      deps.toaster.success({
        title: kind === 'calibration' ? 'Snapshots cleared' : 'Overlays cleared',
        description: 'Removed stored media files.'
      });
    } catch (err) {
      deps.reportError({
        title: 'Delete failed',
        error: err,
        fallback: 'Unable to delete calibration media right now.'
      });
    } finally {
      state.calibrationDeleting = false;
    }
  }

  return {
    refreshIpaStatus,
    applyIpaCcm,
    openCalibrationPreview,
    closeCalibrationPreview,
    openIpaChartSolverForImage,
    closeIpaChartSolver,
    addIpaChartCorner,
    solveIpaChartCcm,
    setGuidedCalibrationMode,
    resetGuidedCalibrationCoverage,
    setCalibrationOwnPhotosOnly,
    refreshCalibrationImages,
    refreshCalibrationImportSources,
    takeCalibrationSnapshot,
    deleteCalibrationSnapshot,
    deleteCalibrationOverlays: () => deleteCalibrationMedia('calibration_overlay'),
    solveCalibration,
    saveSolvedCalibration,
    copyCalibrationFromSelectedStream,
    importCalibrationFromJsonFile
  };
}
