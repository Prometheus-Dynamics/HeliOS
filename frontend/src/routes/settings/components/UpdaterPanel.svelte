<script lang="ts">
  import { apiUrl, type MediaItem, type UpdateAckResponse, type UpdateStateResponse, type UploadUpdateResponse } from '$lib/api/client';
  import { normalizeUploadError, uploadSizeHeaders, verifyUploadedBytes } from '$lib/api/uploadIntegrity';
  import { apiFetch, REQUESTED_BY, uploadOtaImage } from '../api';
  import { createDomainResource } from '$lib/api/domainResources';
  import { subscribeDomainInvalidations } from '$lib/api/invalidation';
  import { startRefreshScheduler } from '$lib/api/refreshScheduler';
  import { buildErrorMessage } from '$lib/ui/errorPolicy';
  import { connectUpdaterStream } from '$lib/api/otaUpdates';
  import { realtimeUpdateMatchesKind, type RealtimeUpdateEvent } from '$lib/api/realtimeUpdates';
  import { onDestroy, onMount } from 'svelte';
  import UpdaterApplyPanel from './UpdaterApplyPanel.svelte';
  import UpdaterConfirmDialog from './UpdaterConfirmDialog.svelte';
  import UpdaterSourceSelector from './UpdaterSourceSelector.svelte';
  import UpdaterStatePanel from './UpdaterStatePanel.svelte';
  import UpdaterSummaryGrid from './UpdaterSummaryGrid.svelte';

  type ImageSourceKind = 'upload' | 'media' | 'url';

  type UpdaterArtifact = {
    url?: string;
    checksum?: string;
    size_bytes?: number;
  };

  type UpdaterState = {
    update_id?: string;
    stage?: string;
    progress_percent?: number;
    started_at?: string;
    finished_at?: string;
    last_error?: string;
    artifacts?: UpdaterArtifact[];
  };

  // Upload
  let sourceKind = $state<ImageSourceKind>('upload');
  let uploadTarget = $state<File | null>(null);
  let uploadBusy = $state(false);
  let uploadError = $state<string | null>(null);
  let uploadStatus = $state<string | null>(null);
  let uploadInfo = $state<UploadUpdateResponse | null>(null);

  // Apply
  let applyBusy = $state(false);
  let applyError = $state<string | null>(null);
  let applyStatus = $state<string | null>(null);
  let applyConfirmOpen = $state(false);
  let applyConfirmChecked = $state(false);
  let deleteImageAfterApply = $state(true);

  // State + info
  let stateLoading = $state(false);
  let stateError = $state<string | null>(null);
  let currentState = $state<UpdaterState | null>(null);
  let cacheUsageBytes = $state<number | null>(null);
  let streamConnected = $state(false);
  let streamClose: (() => void) | null = null;
  let streamReconnectHandle: ReturnType<typeof setTimeout> | null = null;
  let streamNonce = 0;
  let stopStatePollingLoop: (() => void) | null = null;
  let liveRefreshHandle: ReturnType<typeof setTimeout> | null = null;
  let liveRefreshMediaPending = false;
  let liveRefreshStatePending = false;

  // Source selection
  let imageUrlOverride = $state('');
  let mediaLoading = $state(false);
  let mediaError = $state<string | null>(null);
  let mediaItems = $state<MediaItem[]>([]);
  let selectedMedia = $state<string>('');
  let mediaDeleteBusy = $state(false);

  const MEDIA_CACHE_KEY = 'media:updater:v1';
  const MEDIA_CACHE_STALE_MS = 10_000;
  const MEDIA_CACHE_MAX_MS = 120_000;
  const mediaResource = createDomainResource({
    key: MEDIA_CACHE_KEY,
    loader: () => apiFetch<MediaItem[]>('/media'),
    staleMs: MEDIA_CACHE_STALE_MS,
    maxAgeMs: MEDIA_CACHE_MAX_MS,
    kinds: ['media']
  });

  const apiBaseLabel = $derived(apiUrl(''));

  const filteredMedia = $derived(mediaItems.filter((item) => /\.(img|tar\.gz|zip|bin|xz)$/i.test(item.name || '')));

  const selectedMediaUrl = $derived(selectedMedia ? apiUrl(`/media/${encodeURIComponent(selectedMedia)}`) : '');

  const imageUrl = $derived((() => {
    if (sourceKind === 'url') return imageUrlOverride.trim();
    if (sourceKind === 'media') return selectedMediaUrl;
    return uploadInfo?.image_url ?? '';
  })());

  const imageSourceLabel = $derived((() => {
    if (sourceKind === 'url') return imageUrlOverride.trim() ? 'Custom image URL' : 'Custom image URL (missing)';
    if (sourceKind === 'media') return selectedMedia ? `Media file (${selectedMedia})` : 'Media file (not selected)';
    return uploadInfo ? `Uploaded image (${uploadInfo.filename})` : 'Upload (no image uploaded yet)';
  })());

  const stageLabel = $derived((() => {
    const stage = currentState?.stage ?? null;
    if (!stage) return stateLoading ? 'Loading…' : 'Idle';
    const normalized = stage.toString();
    const byValue: Record<string, string> = {
      idle: 'Idle',
      downloading: 'Downloading update',
      verifying: 'Verifying update',
      awaiting_window: 'Pending apply',
      applying: 'Applying',
      rebooting: 'Rebooting',
      complete: 'Complete',
      rolled_back: 'Canceled / Rolled back'
    };
    return byValue[normalized] ?? normalized;
  })());

  const isUpdateActive = $derived(!!currentState?.update_id);
  const isStageInProgress = $derived(currentState?.stage === 'downloading' || currentState?.stage === 'verifying');
  const isApplyInProgress = $derived(currentState?.stage === 'applying' || currentState?.stage === 'rebooting');
  const canApply = $derived(!uploadBusy && !applyBusy && !isApplyInProgress && !!imageUrl);
  const canCancel = $derived(!uploadBusy && !applyBusy && isUpdateActive && !isApplyInProgress);

  const stateDetail = $derived((() => {
    const stage = currentState?.stage;
    const lastError = currentState?.last_error;
    if (stage === 'rebooting') return 'Update finished. Device is rebooting now.';
    if (stage === 'complete') return 'Update complete. Waiting for device reboot.';
    if (stage === 'rolled_back') {
      return lastError ? `Rolled back: ${lastError}` : 'Update rolled back.';
    }
    return applyStatus ?? 'No actions yet';
  })());

  const stateNotice = $derived.by<{ message: string; tone: 'success' | 'warning' | 'error' } | null>(() => {
    const stage = currentState?.stage;
    const lastError = currentState?.last_error;
    if (stage === 'rebooting') {
      return { message: 'Update finished. Device is rebooting now.', tone: 'warning' };
    }
    if (stage === 'complete') {
      return { message: 'Update complete. Waiting for device reboot.', tone: 'success' };
    }
    if (stage === 'rolled_back') {
      return { message: lastError ? `Update rolled back: ${lastError}` : 'Update rolled back.', tone: 'error' };
    }
    return null;
  });

  const summary = $derived((() => {
    const build = '—';
    const stateLabel = stageLabel;
    const sourceDetail = imageUrl || '—';
    const checksumLabel = uploadInfo?.sha256?.slice(0, 12) ?? '—';
    return [
      { label: 'Build', value: build, detail: `API ${apiBaseLabel}` },
      { label: 'Source', value: imageSourceLabel, detail: sourceDetail },
      { label: 'Hash', value: checksumLabel, detail: uploadInfo?.sha256 ? 'SHA256' : 'No hash yet' },
      { label: 'State', value: stateLabel, detail: stateDetail }
    ];
  })());

  function normalizeState(raw: unknown): UpdaterState | null {
    if (!raw || typeof raw !== 'object') return null;
    const obj = raw as Record<string, unknown>;

    const update_id = typeof obj.update_id === 'string' ? obj.update_id : undefined;
    const stage = typeof obj.stage === 'string' ? obj.stage : undefined;
    const progress_percent = typeof obj.progress_percent === 'number' ? obj.progress_percent : undefined;
    const started_at = typeof obj.started_at === 'string' ? obj.started_at : undefined;
    const finished_at = typeof obj.finished_at === 'string' ? obj.finished_at : undefined;
    const last_error = typeof obj.last_error === 'string' ? obj.last_error : undefined;

    const artifacts = Array.isArray(obj.artifacts)
      ? obj.artifacts.map((item) => {
          const a = item as Record<string, unknown>;
          return {
            url: typeof a.url === 'string' ? a.url : undefined,
            checksum: typeof a.checksum === 'string' ? a.checksum : undefined,
            size_bytes: typeof a.size_bytes === 'number' ? a.size_bytes : undefined
          } satisfies UpdaterArtifact;
        })
      : undefined;

    return { update_id, stage, progress_percent, started_at, finished_at, last_error, artifacts };
  }

  function applyStageProgress(updateId: string, percent: number): void {
    const nextPercent = Number.isFinite(percent) ? Math.max(0, Math.min(100, percent)) : undefined;
    stateLoading = false;
    if (!currentState) {
      currentState = {
        update_id: updateId,
        stage: 'downloading',
        progress_percent: nextPercent
      };
      return;
    }
    if (currentState.update_id !== updateId) return;
    const resolvedPercent = nextPercent ?? currentState.progress_percent;
    const nextStage = currentState.stage ?? 'downloading';
    currentState = { ...currentState, stage: nextStage, progress_percent: resolvedPercent };
  }

  function applyStageOverride(updateId: string, stage: string, percent?: number, lastError?: string | null): void {
    const nextPercent = typeof percent === 'number' && Number.isFinite(percent) ? Math.max(0, Math.min(100, percent)) : undefined;
    stateLoading = false;
    if (!currentState) {
      currentState = {
        update_id: updateId,
        stage,
        progress_percent: nextPercent,
        last_error: lastError ?? undefined
      };
      return;
    }
    if (currentState.update_id !== updateId) return;
    currentState = {
      ...currentState,
      stage,
      progress_percent: nextPercent ?? currentState.progress_percent,
      last_error: lastError ?? currentState.last_error
    };
  }

  function startStatePolling(): void {
    if (stopStatePollingLoop) return;
    stopStatePollingLoop = startRefreshScheduler(fetchState, {
      intervalMs: 5_000,
      immediate: true,
      enabled: () => !streamConnected
    });
  }

  function stopStatePolling(): void {
    stopStatePollingLoop?.();
    stopStatePollingLoop = null;
  }

  function disconnectStream(): void {
    streamNonce += 1;
    streamConnected = false;
    if (streamReconnectHandle != null) {
      clearTimeout(streamReconnectHandle);
      streamReconnectHandle = null;
    }
    streamClose?.();
    streamClose = null;
  }

  function scheduleStreamReconnect(): void {
    if (streamReconnectHandle != null) return;
    streamReconnectHandle = setTimeout(() => {
      streamReconnectHandle = null;
      connectStream();
    }, 1000);
  }

  function connectStream(): void {
    const nonce = (streamNonce += 1);
    streamConnected = false;
    stateLoading = true;
    stateError = null;
    stopStatePolling();
    if (streamReconnectHandle != null) {
      clearTimeout(streamReconnectHandle);
      streamReconnectHandle = null;
    }
    streamClose?.();
    streamClose = null;

    let opened = false;
    const close = connectUpdaterStream({
      onOpen: () => {
        if (nonce !== streamNonce) return;
        opened = true;
        streamConnected = true;
        stateError = null;
        stopStatePolling();
      },
      onClose: () => {
        if (nonce !== streamNonce) return;
        streamConnected = false;
        startStatePolling();
        scheduleStreamReconnect();
      },
      onError: (message) => {
        if (nonce !== streamNonce) return;
        stateError = message;
        streamConnected = false;
        startStatePolling();
        if (opened) {
          scheduleStreamReconnect();
        }
      },
      onSnapshot: (payload) => {
        if (nonce !== streamNonce) return;
        stateLoading = false;
        stateError = null;
        currentState = normalizeState(payload.state) ?? null;
        cacheUsageBytes = typeof payload.cache_usage_bytes === 'number' ? payload.cache_usage_bytes : null;
      },
      onStageProgress: (payload) => {
        if (nonce !== streamNonce) return;
        applyStageProgress(payload.update_id, payload.percent);
      },
      onStageComplete: (payload) => {
        if (nonce !== streamNonce) return;
        applyStageOverride(payload.update_id, 'awaiting_window', 100);
      },
      onApplyComplete: (payload) => {
        if (nonce !== streamNonce) return;
        applyStageOverride(payload.update_id, 'complete', 100);
      },
      onRollbackTriggered: (payload) => {
        if (nonce !== streamNonce) return;
        applyStageOverride(payload.update_id, 'rolled_back', 0, payload.reason);
      }
    });

    if (!close) {
      stateLoading = false;
      startStatePolling();
      return;
    }

    streamClose = close;
  }

  function shouldApplyLiveUpdate(event: RealtimeUpdateEvent): boolean {
    if (event.path.startsWith('/v1/ota') || event.path.startsWith('/v1/media')) return true;
    if (realtimeUpdateMatchesKind(event, 'api')) return false;
    return (
      realtimeUpdateMatchesKind(event, 'media') ||
      realtimeUpdateMatchesKind(event, 'device') ||
      realtimeUpdateMatchesKind(event, 'settings')
    );
  }

  function scheduleLiveRefresh(event: RealtimeUpdateEvent): void {
    if (event.path.startsWith('/v1/media') || realtimeUpdateMatchesKind(event, 'media')) {
      liveRefreshMediaPending = true;
    }
    if (
      event.path.startsWith('/v1/ota') ||
      realtimeUpdateMatchesKind(event, 'device') ||
      realtimeUpdateMatchesKind(event, 'settings')
    ) {
      liveRefreshStatePending = true;
    }
    if (liveRefreshHandle != null) return;
    liveRefreshHandle = setTimeout(() => {
      liveRefreshHandle = null;
      const shouldRefreshMedia = liveRefreshMediaPending;
      const shouldRefreshState = liveRefreshStatePending;
      liveRefreshMediaPending = false;
      liveRefreshStatePending = false;
      if (shouldRefreshMedia) {
        void fetchMedia({ force: true });
      }
      if (shouldRefreshState) {
        void fetchState();
      }
    }, 350);
  }

  function handleFileChange(event: Event): void {
    const input = event.target as HTMLInputElement;
    uploadTarget = input.files?.[0] ?? null;
    sourceKind = 'upload';
    uploadError = null;
    uploadStatus = null;
  }

  async function uploadImage(): Promise<void> {
    if (!uploadTarget) {
      uploadError = 'Select an OS image first.';
      return;
    }
    uploadBusy = true;
    uploadError = null;
    uploadStatus = null;
    uploadInfo = null;

    try {
      let payload: UploadUpdateResponse;
      try {
        payload = await uploadOtaImage(uploadTarget, uploadSizeHeaders(uploadTarget));
      } catch (error) {
        throw normalizeUploadError(error, 'Update upload');
      }
      verifyUploadedBytes(uploadTarget.size, payload.size_bytes, 'Update upload');
      uploadInfo = payload;
      uploadStatus = `Uploaded ${payload.filename}`;
      sourceKind = 'upload';
      selectedMedia = payload.filename;
      imageUrlOverride = '';
      applyStatus = null;
      await fetchMedia({ force: true });
      if (!streamConnected) {
        await fetchState();
      }
    } catch (err) {
      uploadError = buildErrorMessage({ error: normalizeUploadError(err, 'Update upload'), fallback: 'Upload failed.' });
    } finally {
      uploadBusy = false;
    }
  }

  async function applyUpdate(): Promise<void> {
    if (!imageUrl) {
      applyError = 'Select a valid update image source first.';
      return;
    }
    applyBusy = true;
    applyError = null;
    applyStatus = null;
    try {
      const payload: { requested_by: string; image_url?: string; size_bytes?: number; checksum?: string; delete_image_after_apply: boolean } = {
        requested_by: REQUESTED_BY,
        delete_image_after_apply: deleteImageAfterApply
      };
      payload.image_url = imageUrl;
      if (sourceKind === 'upload' && uploadInfo) {
        payload.size_bytes = uploadInfo.size_bytes;
        payload.checksum = uploadInfo.sha256;
      }
      const response = await apiFetch<UpdateAckResponse>(
        '/ota/apply',
        { method: 'POST', body: payload },
        { timeoutMs: 150_000 }
      );
      applyStatus = response.message || 'Apply scheduled';
      if (!streamConnected) {
        await fetchState();
      }
    } catch (err) {
      applyError = buildErrorMessage({ error: err, fallback: 'Unable to apply the update.' });
    } finally {
      applyBusy = false;
    }
  }

  function openApplyConfirm(): void {
    applyConfirmChecked = false;
    applyConfirmOpen = true;
  }

  function closeApplyConfirm(): void {
    applyConfirmOpen = false;
  }

  function confirmApply(): void {
    if (!applyConfirmChecked) return;
    applyConfirmOpen = false;
    void applyUpdate();
  }

  async function cancelUpdate(): Promise<void> {
    if (!currentState?.update_id) {
      applyError = 'No active update to cancel.';
      return;
    }
    if (!confirm(`Cancel update ${currentState.update_id}?`)) return;

    applyBusy = true;
    applyError = null;
    applyStatus = null;
    try {
      const response = await apiFetch<UpdateAckResponse>('/ota/cancel', {
        method: 'POST',
        body: { update_id: currentState.update_id, requested_by: REQUESTED_BY }
      });
      applyStatus = response.message || 'Update canceled';
      if (!streamConnected) {
        await fetchState();
      }
    } catch (err) {
      applyError = buildErrorMessage({ error: err, fallback: 'Unable to cancel the update.' });
    } finally {
      applyBusy = false;
    }
  }

  async function fetchState(): Promise<void> {
    stateLoading = true;
    stateError = null;
    try {
      const payload = await apiFetch<UpdateStateResponse>('/ota/state');
      currentState = normalizeState(payload.state) ?? null;
      cacheUsageBytes = typeof payload.cache_usage_bytes === 'number' ? payload.cache_usage_bytes : null;
    } catch (err) {
      stateError = buildErrorMessage({ error: err, fallback: 'Unable to load updater state.' });
      currentState = null;
    } finally {
      stateLoading = false;
    }
  }

  async function fetchMedia(options: { force?: boolean } = {}): Promise<void> {
    mediaLoading = true;
    mediaError = null;
    try {
      const cached = mediaResource.read();
      if (cached?.data?.length) {
        mediaItems = cached.data;
      }
      mediaItems = await mediaResource.refresh({ force: options.force });
    } catch (err) {
      mediaError = buildErrorMessage({ error: err, fallback: 'Unable to load media files.' });
      mediaItems = [];
      mediaResource.invalidate();
    } finally {
      mediaLoading = false;
    }
  }

  async function deleteMediaFile(name: string): Promise<void> {
    if (!name) return;
    if (!confirm(`Remove ${name}? This deletes the stored file from the device.`)) return;
    mediaDeleteBusy = true;
    mediaError = null;
    try {
      await apiFetch(`/media/${encodeURIComponent(name)}`, { method: 'DELETE' });
      if (selectedMedia === name) {
        selectedMedia = '';
        if (sourceKind === 'media') {
          sourceKind = 'upload';
        }
      }
      if (uploadInfo?.filename === name) {
        uploadInfo = null;
        uploadStatus = 'Upload removed';
      }
      await fetchMedia({ force: true });
    } catch (err) {
      mediaError = buildErrorMessage({ error: err, fallback: 'Unable to delete media file.' });
    } finally {
      mediaDeleteBusy = false;
    }
  }

  onMount(() => {
    connectStream();
    return subscribeDomainInvalidations(['media', 'device', 'settings'], (event) => {
      if (!shouldApplyLiveUpdate(event)) return;
      scheduleLiveRefresh(event);
    });
  });

  onDestroy(() => {
    if (liveRefreshHandle != null) {
      clearTimeout(liveRefreshHandle);
      liveRefreshHandle = null;
    }
    liveRefreshMediaPending = false;
    liveRefreshStatePending = false;
    disconnectStream();
    stopStatePolling();
  });

  $effect(() => {
    void fetchMedia();
  });

  $effect(() => {
    if (selectedMedia) sourceKind = 'media';
  });

  $effect(() => {
    if (imageUrlOverride.trim()) sourceKind = 'url';
  });
</script>

<div class="space-y-4 text-sm text-surface-200">
  <UpdaterSummaryGrid items={summary} />

  <UpdaterSourceSelector
    sourceKind={sourceKind}
    imageSourceLabel={imageSourceLabel}
    uploadTarget={uploadTarget}
    uploadBusy={uploadBusy}
    uploadError={uploadError}
    uploadStatus={uploadStatus}
    uploadInfo={uploadInfo}
    mediaLoading={mediaLoading}
    mediaError={mediaError}
    filteredMedia={filteredMedia}
    selectedMedia={selectedMedia}
    selectedMediaUrl={selectedMediaUrl}
    mediaDeleteBusy={mediaDeleteBusy}
    imageUrlOverride={imageUrlOverride}
    onSourceKindChange={(kind) => (sourceKind = kind)}
    onFileChange={handleFileChange}
    onUploadImage={() => void uploadImage()}
    onRefreshMedia={() => void fetchMedia({ force: true })}
    onDeleteMedia={(name) => void deleteMediaFile(name)}
    onSelectMedia={(name) => (selectedMedia = name)}
    onImageUrlChange={(value) => (imageUrlOverride = value)}
  />

  <UpdaterApplyPanel
    updateId={currentState?.update_id ?? null}
    applyError={applyError}
    applyStatus={applyStatus}
    stateNotice={stateNotice}
    canApply={canApply}
    canCancel={canCancel}
    applyBusy={applyBusy}
    isStageInProgress={isStageInProgress}
    imageUrl={imageUrl}
    deleteImageAfterApply={deleteImageAfterApply}
    onOpenConfirm={openApplyConfirm}
    onCancelUpdate={() => cancelUpdate()}
    onDeleteImageAfterApplyChange={(enabled) => (deleteImageAfterApply = enabled)}
  />

  <UpdaterStatePanel
    currentState={currentState}
    stageLabel={stageLabel}
    stateError={stateError}
    stateLoading={stateLoading}
    cacheUsageBytes={cacheUsageBytes}
  />
</div>

<UpdaterConfirmDialog
  open={applyConfirmOpen}
  checked={applyConfirmChecked}
  busy={applyBusy}
  onClose={closeApplyConfirm}
  onToggle={(checked) => (applyConfirmChecked = checked)}
  onConfirm={confirmApply}
/>
