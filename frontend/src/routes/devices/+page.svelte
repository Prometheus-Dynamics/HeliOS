<script lang="ts">
  import { browser } from '$app/environment';
  import { onDestroy, onMount } from 'svelte';
  import type { PageData } from './$types';
  import type { DevicesPageData } from '$lib/types/devices';
  import RegisterCameraModal from '$lib/components/RegisterCameraModal.svelte';
  import SensorViewerModal from '$lib/components/SensorViewerModal.svelte';
  import { toaster } from '$lib/toaster';
  import type { CameraRow } from '$lib/components/devices/DevicesCamerasPanel.svelte';
  import DevicesPageSidebar from '$lib/features/devices/page/DevicesPageSidebar.svelte';
  import DevicesPageContent from '$lib/features/devices/page/DevicesPageContent.svelte';
  import DevicesUnregisterDialog from '$lib/features/devices/page/DevicesUnregisterDialog.svelte';
  import type { ResourceGuardStatus } from '$lib/api/deviceStatusResources';
  import { connectionState } from '$lib/api/connection';
  import { resourceTelemetryStore, type ResourceSample } from '$lib/api/telemetry';
  import type { IconDefinition } from '@fortawesome/free-solid-svg-icons';
  import { faClock, faLayerGroup, faPlug, faSatelliteDish, faTriangleExclamation } from '@fortawesome/free-solid-svg-icons';
  import { registerCameraModal } from '$lib/stores/modals';
  import { createDevicesUiStore } from '$lib/features/devices/store';
  import { buildThrottleBanner, normalizePeripheralToken } from '$lib/features/devices/utils';
  import type { LocalizationConfig } from '$lib/features/localization/localizationConfig';
  import { PROFILE_COLORS, profileColorForId } from '$lib/features/localization/utils';
  import { SvelteMap, SvelteSet } from 'svelte/reactivity';
  import {
    createDevicesPageSupport,
    type PeripheralRow,
    type StreamActionBusyState
  } from './devicesPageSupport';

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
  let hasLoadedOnce = $state(
    Boolean(
      readInitialDevices().summary?.length ||
        readInitialDevices().cameras?.length ||
        readInitialDevices().peripherals?.length
    )
  );

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
  let stopDevicesPageSupport: (() => void) | null = null;

  const devicesPageSupport = createDevicesPageSupport({
    readDevices: () => devices,
    setDevices: (value) => {
      devices = value;
    },
    readHasLoadedOnce: () => hasLoadedOnce,
    setHasLoadedOnce: (value) => {
      hasLoadedOnce = value;
    },
    readIsRefreshing: () => isRefreshing,
    setIsRefreshing: (value) => {
      isRefreshing = value;
    },
    setCamerasLoading: (value) => {
      camerasLoading = value;
    },
    setPeripheralsLoading: (value) => {
      peripheralsLoading = value;
    },
    setLocalizationConfig: (value) => {
      localizationConfig = value;
    },
    setResourceGuardStatus: (value) => {
      resourceGuardStatus = value;
    },
    setLoadError: (value) => {
      loadError = value;
    },
    setFetchedAt: (value) => {
      fetchedAt = value;
    },
    readConnectionStatus: () => connectionStatus,
    readIsBackendUnavailable: () => isBackendUnavailable,
    syncActivePeripheral,
    readStreamActionBusy: () => streamActionBusy,
    setStreamActionBusy: (value) => {
      streamActionBusy = value;
    },
    readPendingUnregister: () => pendingUnregister,
    setPendingUnregister: (value) => {
      pendingUnregister = value;
    }
  });

  onMount(() => {
    stopDevicesPageSupport = devicesPageSupport.start();
  });

  onDestroy(() => {
    stopDevicesPageSupport?.();
    stopDevicesPageSupport = null;
    devicesUiStore.destroy();
  });

  $effect(() => {
    devicesUiStore.update(cameras, peripherals);
  });

  function isResourceGuardDegraded(camera: CameraRow): boolean {
    const streamId = camera.captureSessionId?.toString().trim();
    if (!streamId) return false;
    return resourceGuardDegradedStreamIds.has(streamId);
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
      onDownloadManifest={(camera) => void devicesPageSupport.downloadCameraManifest(camera as CameraRow)}
      onRequestUnregister={(camera) => devicesPageSupport.requestUnregister(camera as CameraRow)}
      onRestoreStreamResources={(camera) => void devicesPageSupport.restoreCameraStreamResources(camera as CameraRow)}
      actionIsBusy={(id, action) => devicesPageSupport.actionIsBusy(id, action)}
      sessionRefFromCamera={(camera) => devicesPageSupport.sessionRefFromCamera(camera as CameraRow)}
      isResourceGuardDegraded={(camera) => isResourceGuardDegraded(camera as CameraRow)}
    />
  </div>

	  <SensorViewerModal
	    peripheral={activePeripheral}
	    on:close={handlePeripheralClose}
	    on:calibrate={handlePeripheralCalibrate}
	    on:refresh={() => void devicesPageSupport.refreshDevices()}
	  />

    {#if $registerCameraModal}
      <RegisterCameraModal
        registeredIds={registeredIds}
        registeredHardwareIds={registeredHardwareIds}
        on:create={() => void devicesPageSupport.refreshDevices()}
      />
    {/if}

    <DevicesUnregisterDialog
      pending={pendingUnregister}
      busy={pendingUnregister ? devicesPageSupport.actionIsBusy(pendingUnregister.id, 'unregister') : false}
      onClose={devicesPageSupport.closeUnregisterDialog}
      onConfirm={() => void devicesPageSupport.confirmUnregister()}
    />
</section>
