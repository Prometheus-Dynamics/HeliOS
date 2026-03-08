<script lang="ts">
  import { browser } from '$app/environment';
  import { onDestroy, onMount } from 'svelte';
  import type { PageData } from './$types';
  import type { DevicesPageData } from '$lib/types/devices';
  import { RegisterCameraModal, SensorViewerModal, toaster } from '$lib';
  import type { CameraRow } from '$lib';
  import DevicesPageSidebar from '$lib/features/devices/page/DevicesPageSidebar.svelte';
  import DevicesPageContent from '$lib/features/devices/page/DevicesPageContent.svelte';
  import DevicesUnregisterDialog from '$lib/features/devices/page/DevicesUnregisterDialog.svelte';
  import { connectDevicesUpdatesStream } from '$lib/api/devicesUpdates';
  import { StreamsApi } from '$lib/api/streamsApi';
  import { apiFetch } from '$lib/api/apiFetch';
  import { connectionState } from '$lib/api/connection';
  import { resourceTelemetryStore, type ResourceSample } from '$lib/api/telemetry';
  import { invalidateSWR, invalidateSWRPrefix } from '$lib/utils/swrCache';
  import { createRefreshableResource } from '$lib/utils/refreshableResource';
  import { buildErrorMessage, reportError } from '$lib/ui/errorPolicy';
  import { createBackoffTimer } from '$lib/utils/backoff';
  import type { IconDefinition } from '@fortawesome/free-solid-svg-icons';
  import { faClock, faLayerGroup, faPlug, faSatelliteDish, faTriangleExclamation } from '@fortawesome/free-solid-svg-icons';
  import { fetchDevicesCamerasSnapshot, fetchDevicesPeripheralsSnapshot } from '$lib/api/devicesPage';
  import { registerCameraModal } from '$lib/stores/modals';
  import { createDevicesUiStore } from '$lib/features/devices/store';
  import { buildThrottleBanner, normalizePeripheralToken } from '$lib/features/devices/utils';
  import { fetchLocalizationConfig, type LocalizationConfig } from '$lib/features/localization/localizationConfig';
  import { PROFILE_COLORS, profileColorForId } from '$lib/features/localization/utils';
  import { SvelteMap, SvelteSet } from 'svelte/reactivity';

  const AUTO_REFRESH_MS = 10_000;
  const DEVICES_CAMERAS_CACHE_KEY = 'devices:cameras:v1';
  const DEVICES_PERIPHERALS_CACHE_KEY = 'devices:peripherals:v1';
  const DEVICES_CACHE_STALE_MS = 10_000;
  const DEVICES_CACHE_MAX_MS = 120_000;
  const WS_REFRESH_DEBOUNCE_MS = 250;
  const DEVICES_UPDATES_RECONNECT_MS = 750;
  const DEVICES_UPDATES_RECONNECT_MAX_MS = 15_000;
  const LOCALIZATION_CONFIG_REFRESH_MS = 30_000;
  const devicesCamerasResource = createRefreshableResource({
    key: DEVICES_CAMERAS_CACHE_KEY,
    loader: fetchDevicesCamerasSnapshot,
    staleMs: DEVICES_CACHE_STALE_MS,
    maxAgeMs: DEVICES_CACHE_MAX_MS
  });
  const devicesPeripheralsResource = createRefreshableResource({
    key: DEVICES_PERIPHERALS_CACHE_KEY,
    loader: fetchDevicesPeripheralsSnapshot,
    staleMs: DEVICES_CACHE_STALE_MS,
    maxAgeMs: DEVICES_CACHE_MAX_MS
  });

  const { data } = $props<{ data: PageData }>();
  const readInitialDevices = () => data.initial;
  const readReceivedAt = () => data.receivedAt;
  let devices = $state<DevicesPageData>(clone(readInitialDevices()));
  let loadError = $state<string | null>(null);
  let isRefreshing = $state(false);
  let camerasLoading = $state(false);
  let peripheralsLoading = $state(false);
  let localizationConfig = $state<LocalizationConfig | null>(null);
  let fetchedAt = $state<number>(readReceivedAt());
  let hasLoadedOnce = $state(false);

  let refreshTimer: number | null = null;
  let resourceGuardTimer: number | null = null;
  let localizationRefreshTimer: number | null = null;
  let wsRefreshTimer: number | null = null;
  let updatesCleanup: (() => void) | null = null;
  let updatesNonce = 0;
  let updatesReconnectPending = false;
  const updatesReconnectBackoff = createBackoffTimer({
    baseMs: DEVICES_UPDATES_RECONNECT_MS,
    maxMs: DEVICES_UPDATES_RECONNECT_MAX_MS
  });

  const cameras = $derived(devices.cameras ?? []);
  const peripherals = $derived(devices.peripherals ?? []);
  const deviceErrors = $derived(devices.errors ?? {});
  const peripheralError = $derived(deviceErrors.peripherals ?? null);
  const connectionStatus = $derived($connectionState.status);
  const isBackendUnavailable = $derived(browser && navigator.onLine === false);
  const telemetrySample = $derived($resourceTelemetryStore as ResourceSample);
  const throttleBanner = $derived(buildThrottleBanner(telemetrySample));
  const camerasInitialLoading = $derived(camerasLoading && cameras.length === 0);

  type CameraStatus = DevicesPageData['cameras'][number]['status'];
  type PeripheralRow = DevicesPageData['peripherals'][number];
  type CameraFilterOption = 'all' | CameraStatus;
  type LocalizationProfileBadge = { id: string; name: string; color: string };

  const CAMERA_FILTERS: Array<{ id: CameraFilterOption; label: string; description: string; icon: IconDefinition }> = [
    { id: 'all', label: 'All streams', description: 'Registered capture sessions', icon: faLayerGroup },
    { id: 'live', label: 'Live streams', description: 'Actively ingesting frames', icon: faSatelliteDish },
    { id: 'degraded', label: 'Degraded', description: 'Needs an operator check', icon: faTriangleExclamation },
    { id: 'idle', label: 'Idle', description: 'Ready for assignment', icon: faClock },
    { id: 'offline', label: 'Offline', description: 'Not reachable right now', icon: faPlug }
  ];

  const CAMERA_FILTER_LOOKUP = Object.fromEntries(CAMERA_FILTERS.map((filter) => [filter.id, filter])) as Record<CameraFilterOption, (typeof CAMERA_FILTERS)[number]>;

  let searchQuery = $state('');
  let cameraFilter = $state<CameraFilterOption>('all');

  const hasActiveFilters = $derived((() => cameraFilter !== 'all' || searchQuery.trim().length > 0)());

  const devicesUiStore = createDevicesUiStore();
  const cameraCards = devicesUiStore.cameraCards;
  const peripheralItems = devicesUiStore.peripheralItems;
  const cameraStatusCounts = devicesUiStore.cameraStatusCounts;
  let activePeripheral = $state<PeripheralRow | null>(null);
  let activePeripheralId = $state<string | null>(null);
  let activePeripheralHardwareId = $state<string | null>(null);
  let activePeripheralType = $state<string | null>(null);
  let activePeripheralName = $state<string | null>(null);
  let activePeripheralNamespace = $state<string | null>(null);

  const statusTiles = $derived([
    { label: 'Live', value: String($cameraStatusCounts.live ?? 0) },
    { label: 'Degraded', value: String($cameraStatusCounts.degraded ?? 0) },
    { label: 'Idle', value: String($cameraStatusCounts.idle ?? 0) },
    { label: 'Offline', value: String($cameraStatusCounts.offline ?? 0) }
  ]);

  const profileIndexById = $derived.by(() => {
    const indexById = new SvelteMap<string, number>();
    const profiles = localizationConfig?.profiles ?? [];
    profiles.forEach((profile, idx) => {
      indexById.set(profile.id, idx);
    });
    return indexById;
  });

  const localizationProfilesByStreamKey = $derived.by<Record<string, LocalizationProfileBadge[]>>(() => {
    const out: Record<string, LocalizationProfileBadge[]> = {};
    const profiles = localizationConfig?.profiles ?? [];
    for (const profile of profiles) {
      const name = (profile.name ?? '').trim() || profile.id;
      const color = profileColorForId(profile.id, profiles, profileIndexById, PROFILE_COLORS);
      for (const source of profile.sources ?? []) {
        if (!source.enabled) continue;
        const keys = [String(source.streamId ?? '').trim(), String(source.cameraUid ?? '').trim()].filter(Boolean);
        for (const key of keys) {
          const current = out[key] ?? [];
          if (current.some((entry) => entry.id === profile.id)) continue;
          out[key] = [...current, { id: profile.id, name, color }];
        }
      }
    }
    for (const [key, badges] of Object.entries(out)) {
      out[key] = badges.sort((left, right) => left.name.localeCompare(right.name));
    }
    return out;
  });

  function cameraProfileLookupKeys(camera: {
    captureSessionId?: string | null;
    captureSessionAlias?: string | null;
    cameraUid?: string | null;
  }): string[] {
    const out = new SvelteSet<string>();
    for (const value of [camera.captureSessionId, camera.captureSessionAlias, camera.cameraUid]) {
      const trimmed = String(value ?? '').trim();
      if (trimmed) out.add(trimmed);
    }
    return Array.from(out);
  }

  function localizationProfilesForCamera(camera: {
    captureSessionId?: string | null;
    captureSessionAlias?: string | null;
    cameraUid?: string | null;
  }): LocalizationProfileBadge[] {
    const badgesById = new SvelteMap<string, LocalizationProfileBadge>();
    for (const key of cameraProfileLookupKeys(camera)) {
      const matches = localizationProfilesByStreamKey[key] ?? [];
      for (const match of matches) {
        badgesById.set(match.id, match);
      }
    }
    return Array.from(badgesById.values()).sort((left, right) => left.name.localeCompare(right.name));
  }

  const cameraLocalizationProfilesById = $derived.by<Record<string, LocalizationProfileBadge[]>>(() => {
    const out: Record<string, LocalizationProfileBadge[]> = {};
    for (const camera of $cameraCards) {
      out[camera.id] = localizationProfilesForCamera(camera);
    }
    return out;
  });

  const filteredCameraCards = $derived((() => {
    const query = searchQuery.trim().toLowerCase();
    return $cameraCards.filter((camera) => {
      const matchesFilter = cameraFilter === 'all' ? true : camera.status === cameraFilter;
      if (!matchesFilter) return false;
      if (!query.length) return true;
      const profileNames = (cameraLocalizationProfilesById[camera.id] ?? []).map((profile) => profile.name).join(' ');
      const haystack = `${camera.name} ${camera.pipeline ?? ''} ${camera.driverNamespace ?? ''} ${profileNames}`.toLowerCase();
      return haystack.includes(query);
    });
  })());

  const cameraEmptyMessage = $derived((() => {
    if (!cameras.length) {
      return 'No streams are registered yet.';
    }
    if (filteredCameraCards.length === 0) {
      const trimmedQuery = searchQuery.trim();
      if (trimmedQuery.length) {
        return `No cameras match “${trimmedQuery}”.`;
      }
      if (cameraFilter !== 'all') {
        const label = CAMERA_FILTER_LOOKUP[cameraFilter]?.label ?? 'selected';
        return `No ${label.toLowerCase()} streams found.`;
      }
      return 'No streams match the current filters.';
    }
    return 'No streams are registered yet.';
  })());


  const registeredHardwareIds = $derived<string[]>([
    ...new SvelteSet(
      cameras
        .map((cam) => cam.hardwareId?.trim())
        .filter((value): value is string => Boolean(value))
    )
  ]);
  const registeredIds = $derived<string[]>([
    ...new SvelteSet(
      cameras
        .flatMap((cam) => [
          cam.captureSessionId?.toString().trim(),
          cam.driverCameraId?.trim(),
          cam.cameraUid?.trim()
        ])
        .filter((value): value is string => Boolean(value))
    )
  ]);

	  type StreamActionBusyState = {
	    unregister?: boolean;
	    download?: boolean;
      restore?: boolean;
  };

  type ResourceGuardStatus = {
    degraded_streams?: Array<{
      stream_id: string;
    }>;
  };

  let streamActionBusy = $state<Record<string, StreamActionBusyState>>({});
  let resourceGuardStatus = $state<ResourceGuardStatus | null>(null);
  const resourceGuardDegradedStreamIds = $derived.by(() => {
    const ids = new SvelteSet<string>();
    for (const stream of resourceGuardStatus?.degraded_streams ?? []) {
      const id = String(stream?.stream_id ?? '').trim();
      if (id) ids.add(id);
    }
    return ids;
  });
  let pendingUnregister = $state<CameraRow | null>(null);

  onMount(() => {
    if (browser) {
      const cachedCameras = devicesCamerasResource.read();
      if (cachedCameras?.data) {
        devices = { ...devices, cameras: cachedCameras.data };
        fetchedAt = cachedCameras.fetchedAt;
        hasLoadedOnce = true;
      }
      const cachedPeripherals = devicesPeripheralsResource.read();
      if (cachedPeripherals?.data) {
        devices = {
          ...devices,
          peripherals: cachedPeripherals.data.peripherals,
          errors: { ...(devices.errors ?? {}), peripherals: cachedPeripherals.data.error }
        };
        fetchedAt = Math.max(fetchedAt, cachedPeripherals.fetchedAt);
        hasLoadedOnce = true;
      }
    }
    void refreshDevices({ bootstrap: true });
    void refreshLocalizationConfig();
    void refreshResourceGuardStatus();
    connectDevicesUpdates();
    refreshTimer = window.setInterval(() => {
      if (!document.hidden && !isBackendUnavailable) {
        void refreshDevices();
      }
    }, AUTO_REFRESH_MS);
    resourceGuardTimer = window.setInterval(() => {
      if (!document.hidden && !isBackendUnavailable) {
        void refreshResourceGuardStatus();
      }
    }, 4_000);
    localizationRefreshTimer = window.setInterval(() => {
      if (!document.hidden && !isBackendUnavailable) {
        void refreshLocalizationConfig();
      }
    }, LOCALIZATION_CONFIG_REFRESH_MS);
  });

  onDestroy(() => {
	    if (refreshTimer) {
	      clearInterval(refreshTimer);
	      refreshTimer = null;
	    }
    if (resourceGuardTimer) {
      clearInterval(resourceGuardTimer);
      resourceGuardTimer = null;
    }
    if (localizationRefreshTimer) {
      clearInterval(localizationRefreshTimer);
      localizationRefreshTimer = null;
    }
    if (wsRefreshTimer) {
      clearTimeout(wsRefreshTimer);
      wsRefreshTimer = null;
    }
    disconnectDevicesUpdates();
    devicesUiStore.destroy();
  });

  function scheduleWsRefresh(): void {
    if (wsRefreshTimer != null) return;
    wsRefreshTimer = window.setTimeout(() => {
      wsRefreshTimer = null;
      if (document.hidden) return;
      void refreshDevices({ force: true });
    }, WS_REFRESH_DEBOUNCE_MS);
  }

  function scheduleDevicesUpdatesReconnect(): void {
    if (updatesReconnectPending) return;
    updatesReconnectPending = true;
    const delay = updatesReconnectBackoff.bump();
    updatesReconnectBackoff.schedule(() => {
      updatesReconnectPending = false;
      connectDevicesUpdates();
    }, delay);
  }

  function disconnectDevicesUpdates(): void {
    updatesNonce += 1;
    updatesReconnectPending = false;
    updatesReconnectBackoff.cancel();
    updatesCleanup?.();
    updatesCleanup = null;
  }

  function connectDevicesUpdates(): void {
    const nonce = (updatesNonce += 1);
    updatesReconnectPending = false;
    updatesReconnectBackoff.cancel();
    updatesCleanup?.();
    updatesCleanup = null;
    updatesCleanup = connectDevicesUpdatesStream({
      onOpen: () => {
        if (nonce !== updatesNonce) return;
        updatesReconnectBackoff.reset();
      },
      onUpdate: () => {
        if (nonce !== updatesNonce) return;
        updatesReconnectBackoff.reset();
        if (document.hidden) return;
        scheduleWsRefresh();
      },
      onClose: () => {
        if (nonce !== updatesNonce) return;
        scheduleDevicesUpdatesReconnect();
      },
      onError: (message) => {
        if (nonce !== updatesNonce) return;
        if (connectionStatus === 'online') {
          console.warn('Devices updates socket error', message);
        }
        scheduleDevicesUpdatesReconnect();
      }
    });
  }

  $effect(() => {
    devicesUiStore.update(cameras, peripherals);
  });

  async function refreshLocalizationConfig(): Promise<void> {
    try {
      localizationConfig = await fetchLocalizationConfig();
    } catch (error) {
      if (connectionStatus === 'online' && !isAbortError(error)) {
        console.warn('Failed to refresh localization config for devices page', error);
      }
    }
  }

  async function refreshDevices(options: { bootstrap?: boolean; force?: boolean } = {}): Promise<void> {
    if (isRefreshing) return;

    const { bootstrap = false, force = false } = options;
    if (isBackendUnavailable) {
      if (bootstrap && !hasLoadedOnce) {
        loadError = 'Backend unavailable. Update the API base URL in Settings.';
        hasLoadedOnce = true;
      }
      return;
    }
    if (bootstrap) {
      loadError = null;
    }
    isRefreshing = true;
    let pending = 2;
    let failures = 0;
    let sawSuccess = false;

    const finalize = () => {
      pending -= 1;
      if (pending > 0) return;
      isRefreshing = false;
      if (bootstrap || !hasLoadedOnce) {
        hasLoadedOnce = true;
      }
      if (!sawSuccess && failures >= 2) {
        loadError = 'Unable to load devices right now.';
      }
    };

    camerasLoading = true;
    devicesCamerasResource.refresh({ force })
      .then((camerasPayload) => {
        sawSuccess = true;
        loadError = null;
        camerasLoading = false;
        devices = { ...devices, cameras: camerasPayload };
        fetchedAt = Date.now();
      })
      .catch((error) => {
        failures += 1;
        camerasLoading = false;
        if (!isAbortError(error)) {
          if (connectionStatus === 'online') {
            console.error('Failed to refresh device cameras', error);
          }
        }
        devicesCamerasResource.invalidate();
      })
      .finally(finalize);

    peripheralsLoading = true;
    devicesPeripheralsResource.refresh({ force })
      .then((snapshot) => {
        sawSuccess = true;
        loadError = null;
        peripheralsLoading = false;
        devices = {
          ...devices,
          peripherals: snapshot.peripherals,
          errors: { ...(devices.errors ?? {}), peripherals: snapshot.error }
        };
        syncActivePeripheral(snapshot.peripherals);
        fetchedAt = Date.now();
      })
      .catch((error) => {
        failures += 1;
        peripheralsLoading = false;
        if (!isAbortError(error)) {
          if (connectionStatus === 'online') {
            console.error('Failed to refresh device peripherals', error);
          }
          const message = buildErrorMessage({ error, fallback: 'Unable to load peripherals right now.' });
          devices = { ...devices, errors: { ...(devices.errors ?? {}), peripherals: message } };
        }
        devicesPeripheralsResource.invalidate();
      })
      .finally(finalize);
  }

  async function refreshResourceGuardStatus(): Promise<void> {
    try {
      resourceGuardStatus = await apiFetch<ResourceGuardStatus>('/device/resource-guard');
    } catch {
      resourceGuardStatus = null;
    }
  }

  function actionIsBusy(cameraId: string, action: keyof StreamActionBusyState): boolean {
    const entry = streamActionBusy[cameraId];
    if (!entry) return false;
    return Boolean(entry[action]);
  }

  function setActionBusy(cameraId: string, action: keyof StreamActionBusyState, value: boolean): void {
    streamActionBusy = {
      ...streamActionBusy,
      [cameraId]: {
        ...(streamActionBusy[cameraId] ?? {}),
        [action]: value
      }
    };
  }

  function requestUnregister(camera: CameraRow): void {
    if (pendingUnregister || actionIsBusy(camera.id, 'unregister')) return;
    pendingUnregister = camera;
  }

  function closeUnregisterDialog(): void {
    pendingUnregister = null;
  }

  async function confirmUnregister(): Promise<void> {
    const target = pendingUnregister;
    if (!target) return;
    await unregisterCameraStream(target);
    pendingUnregister = null;
  }

  function sessionRefFromCamera(camera: CameraRow): string | null {
    const id = camera.captureSessionId?.toString().trim() ?? '';
    if (id.startsWith('peer:')) return null;
    const alias = camera.captureSessionAlias?.trim();
    if (alias) return alias;
    return id.length ? id : null;
  }

  async function unregisterCameraStream(camera: CameraRow): Promise<void> {
    const streamId = camera.captureSessionId?.toString().trim() ?? '';
    if (!streamId) {
      toaster.error({
        title: 'Stream not active',
        description: 'This camera does not have an active capture session.'
      });
      return;
    }
    if (actionIsBusy(camera.id, 'unregister')) return;
    setActionBusy(camera.id, 'unregister', true);
    try {
      await StreamsApi.deleteStream({ id: streamId });
      toaster.success({
        title: 'Stream deleted',
        description: `${camera.name} capture session removed`
      });
      invalidateSWRPrefix('devices:');
      invalidateSWRPrefix('media:');
      invalidateSWR('media:stream-labels:v1');
      await refreshDevices();
    } catch (error) {
      console.error('Failed to unregister capture session', error);
      reportError({
        title: 'Delete failed',
        error,
        fallback: 'Unable to delete the stream right now.'
      });
    } finally {
      setActionBusy(camera.id, 'unregister', false);
    }
  }

  async function downloadCameraManifest(camera: CameraRow): Promise<void> {
    const streamId = camera.captureSessionId?.toString().trim() ?? '';
    if (!streamId) {
      toaster.error({
        title: 'Manifest unavailable',
        description: 'No capture session is active for this camera.'
      });
      return;
    }
    if (actionIsBusy(camera.id, 'download')) return;
    setActionBusy(camera.id, 'download', true);
    try {
      const response = await StreamsApi.getStream({ id: streamId });
      const manifest = response?.manifest;
      if (!manifest || typeof manifest !== 'object') {
        throw new Error('Manifest not available');
      }
      const pretty = JSON.stringify(manifest, null, 2);
      const blob = new Blob([pretty], { type: 'application/json' });
      const url = URL.createObjectURL(blob);
      const suggestedName =
        typeof manifest === 'object' && manifest && 'path' in manifest && typeof manifest.path === 'string'
          ? manifest.path.split('/').pop() ?? 'stream-manifest'
          : 'stream-manifest';
      const filename = `${suggestedName.replace(/\.json$/i, '') || 'stream-manifest'}-${camera.id}.json`;
      const anchor = document.createElement('a');
      anchor.href = url;
      anchor.download = filename;
      document.body.appendChild(anchor);
      anchor.click();
      document.body.removeChild(anchor);
      URL.revokeObjectURL(url);
      toaster.success({
        title: 'Manifest downloaded',
        description: `${camera.name} manifest saved`
      });
    } catch (error) {
      console.error('Failed to download manifest', error);
      reportError({
        title: 'Download failed',
        error,
        fallback: 'Unable to download the manifest right now.'
      });
    } finally {
      setActionBusy(camera.id, 'download', false);
    }
  }

  function isResourceGuardDegraded(camera: CameraRow): boolean {
    const streamId = camera.captureSessionId?.toString().trim();
    if (!streamId) return false;
    return resourceGuardDegradedStreamIds.has(streamId);
  }

  async function restoreCameraStreamResources(camera: CameraRow): Promise<void> {
    const streamId = camera.captureSessionId?.toString().trim() ?? '';
    if (!streamId) {
      toaster.error({
        title: 'Restore unavailable',
        description: 'This camera does not have an active stream ID.'
      });
      return;
    }
    if (actionIsBusy(camera.id, 'restore')) return;
    setActionBusy(camera.id, 'restore', true);
    try {
      await apiFetch(`/device/resource-guard/restore/${encodeURIComponent(streamId)}`, { method: 'POST' });
      toaster.success({
        title: 'Stream resources re-enabled',
        description: `${camera.name} codecs restored`
      });
      await refreshResourceGuardStatus();
      await refreshDevices({ force: true });
    } catch (error) {
      reportError({
        title: 'Restore failed',
        error,
        fallback: 'Unable to re-enable stream resources right now.'
      });
    } finally {
      setActionBusy(camera.id, 'restore', false);
    }
  }

  function rememberActivePeripheral(peripheral: PeripheralRow | null) {
    activePeripheralId = peripheral?.driverCameraId ?? null;
    activePeripheralHardwareId = peripheral?.hardwareId ?? null;
    activePeripheralType = normalizePeripheralToken(peripheral?.type ?? null);
    activePeripheralName = normalizePeripheralToken(peripheral?.name ?? null);
    activePeripheralNamespace = normalizePeripheralToken(peripheral?.driverNamespace ?? null);
  }

  function openPeripheral(peripheral: PeripheralRow | null) {
    activePeripheral = peripheral;
    rememberActivePeripheral(peripheral);
  }

  function handlePeripheralClose() {
    activePeripheral = null;
    activePeripheralId = null;
    activePeripheralHardwareId = null;
    activePeripheralType = null;
    activePeripheralName = null;
    activePeripheralNamespace = null;
  }

  function handlePeripheralCalibrate(event: CustomEvent<{ peripheral: PeripheralRow }>) {
    const target = event.detail.peripheral;
    toaster.success({
      title: 'Calibration started',
      description: `Calibrating ${target.name}…`
    });
  }

  function syncActivePeripheral(entries: PeripheralRow[]) {
    if (!activePeripheralId && !activePeripheralHardwareId) {
      if (!activePeripheralName) return;
      const updated = entries.find(
        (entry) => normalizePeripheralToken(entry.name ?? null) === activePeripheralName
      );
      if (!updated) return;
      activePeripheral = updated;
      rememberActivePeripheral(updated);
      return;
    }

    const candidates = entries.filter((entry) => {
      if (activePeripheralId && entry.driverCameraId === activePeripheralId) return true;
      if (activePeripheralHardwareId && entry.hardwareId === activePeripheralHardwareId) return true;
      return false;
    });
    if (!candidates.length) return;

    let updated = candidates[0];
    if (candidates.length > 1) {
      const typeMatch = activePeripheralType
        ? candidates.find((entry) => normalizePeripheralToken(entry.type ?? null) === activePeripheralType)
        : null;
      const nameMatch = activePeripheralName
        ? candidates.find((entry) => normalizePeripheralToken(entry.name ?? null) === activePeripheralName)
        : null;
      const namespaceMatch = activePeripheralNamespace
        ? candidates.find(
            (entry) => normalizePeripheralToken(entry.driverNamespace ?? null) === activePeripheralNamespace
          )
        : null;
      updated = typeMatch ?? nameMatch ?? namespaceMatch ?? updated;
    }

    activePeripheral = updated;
    rememberActivePeripheral(updated);
  }

  function clearInventoryFilters() {
    searchQuery = '';
    cameraFilter = 'all';
  }

  function isAbortError(error: unknown): boolean {
    if (!error) return false;
    if (error instanceof DOMException && error.name === 'AbortError') return true;
    if (error instanceof Error && error.name === 'AbortError') return true;
    return (error as { name?: string }).name === 'AbortError';
  }

  function clone<T>(value: T): T {
    return JSON.parse(JSON.stringify(value));
  }
</script>

<section class="flex min-h-0 flex-1 flex-col gap-4 2xl:gap-6">
  {#if loadError}
    <div class="rounded border border-error-500/40 bg-error-500/10 px-4 py-2 text-sm text-error-200">
      {loadError}
    </div>
  {/if}

	  <div class="flex flex-1 flex-col gap-4 2xl:gap-6 lg:min-h-0 lg:flex-row">
    <DevicesPageSidebar
      bind:searchQuery
      bind:cameraFilter
      filters={CAMERA_FILTERS}
      {hasActiveFilters}
      onRegister={() => registerCameraModal.set(true)}
      onClearFilters={clearInventoryFilters}
    />

    <DevicesPageContent
      {throttleBanner}
      {camerasInitialLoading}
      {statusTiles}
      {filteredCameraCards}
      {hasActiveFilters}
      {cameraEmptyMessage}
      cameraLocalizationProfilesById={cameraLocalizationProfilesById}
      peripherals={$peripheralItems}
      {peripheralError}
      {peripheralsLoading}
      onSelectPeripheral={(peripheral) => openPeripheral(peripheral as PeripheralRow | null)}
      onRegisterStream={() => registerCameraModal.set(true)}
      onClearFilters={clearInventoryFilters}
      onDownloadManifest={(camera) => void downloadCameraManifest(camera as CameraRow)}
      onRequestUnregister={(camera) => requestUnregister(camera as CameraRow)}
      onRestoreStreamResources={(camera) => void restoreCameraStreamResources(camera as CameraRow)}
      actionIsBusy={(id, action) => actionIsBusy(id, action)}
      sessionRefFromCamera={(camera) => sessionRefFromCamera(camera as CameraRow)}
      isResourceGuardDegraded={(camera) => isResourceGuardDegraded(camera as CameraRow)}
    />
  </div>

	  <SensorViewerModal
	    peripheral={activePeripheral}
	    on:close={handlePeripheralClose}
	    on:calibrate={handlePeripheralCalibrate}
	    on:refresh={() => void refreshDevices()}
	  />

    {#if $registerCameraModal}
      <RegisterCameraModal
        registeredIds={registeredIds}
        registeredHardwareIds={registeredHardwareIds}
        on:create={() => void refreshDevices()}
      />
    {/if}

    <DevicesUnregisterDialog
      pending={pendingUnregister}
      busy={pendingUnregister ? actionIsBusy(pendingUnregister.id, 'unregister') : false}
      onClose={closeUnregisterDialog}
      onConfirm={() => void confirmUnregister()}
    />
</section>
