<script lang="ts">
  import { onMount } from 'svelte';
  import { toaster } from '$lib';
  import PeersDiscoverModal from '$lib/features/peers/page/PeersDiscoverModal.svelte';
  import PeersSidebar from '$lib/features/peers/page/PeersSidebar.svelte';
  import PeersListPanel from '$lib/features/peers/page/PeersListPanel.svelte';
  import { discoverPhotonvisionStreams, probePeer, registerPeer, syncPeerPipelines } from '$lib/api/peers';
  import { buildErrorMessage, reportError } from '$lib/ui/errorPolicy';
  import type {
    DiscoveredStream,
    PeerIntegrationKind,
    PeerProbeResponse,
    PeerSummary
  } from '$lib/types/peer';
  import { faCamera, faWrench } from '@fortawesome/free-solid-svg-icons';

  import type { CustomMappingPreview, PeerFilterDefinition, PeerFilterOption } from '$lib/features/peers/types';
  import { createPeersStore } from '$lib/features/peers/store';
  import {
    blankMappingForm,
    buildCustomUrl,
    defaultIntegration,
    cloneIntegrationMetadata,
    composeIntegrationFromForm,
    deviceHostFromApiBase,
    displayName,
    isQuickSetupKind,
    mappingFormFromIntegration,
    mappingFromForm,
    managementTarget,
    normalizeDeviceHost,
    parsePort,
    peerSearchableContent,
    presetFor,
    previewCustomMapping
  } from '$lib/features/peers/utils';

  const FILTERS: PeerFilterDefinition[] = [
    { id: 'all', label: 'All peers', description: 'Show every registered device', icon: faCamera },
    { id: 'helios', label: 'HeliOS', description: 'Managed Helios nodes', logo: '/logo.svg' },
    { id: 'limelight_os', label: 'LimelightOS', description: 'Stream-ingest only peers', logo: '/logos/limelight_icon.webp' },
    { id: 'photonvision', label: 'PhotonVision', description: 'PhotonVision localization peers', logo: '/logos/photonvision_logo.png' },
    { id: 'custom', label: 'Custom', description: 'Manually mapped cameras', icon: faWrench },
  ];

  // shared helpers moved to $lib/features/peers/utils

  import type { PageData } from './$types';

  const { data } = $props<{ data: PageData }>();
  const readPayload = () => data.payload;
  const peersStore = createPeersStore(readPayload());
  const pending = peersStore.pending;
  const hasLoadedOnce = peersStore.hasLoadedOnce;
  let loadError = $state<string | null>(readPayload().errorMessage ?? null);
  let customPending = $state({ customSave: false, customTest: false, probe: false });
  let customForm = $state({
    peerId: null as string | null,
    alias: '',
    deviceIp: '',
    apiBaseUrl: '',
    endpointHost: '',
    endpointPort: '',
    managementUrl: '',
    streamUrl: '',
    streamUrls: [] as string[],
    apiEndpoint: '',
    networkTable: '',
    telemetryEndpoint: '',
    integrationKind: 'custom' as PeerIntegrationKind,
    integrationBaseline: defaultIntegration(),
    mapping: blankMappingForm(),
    poseSource: 'http' as 'http' | 'nt',
    arucoSource: 'http' as 'http' | 'nt',
  });
  let lastPresetKey = $state<string | null>(null);
  let photonvisionState = $state<{ host: string | null; pending: boolean; error: string | null; streams: DiscoveredStream[] }>({
    host: null,
    pending: false,
    error: null,
    streams: [],
  });
  let peerProbeCache = $state<Record<string, PeerProbeResponse | null>>({});
  let showManagement = $state(false);
  let showPoseMapping = $state(false);
  let showArucoMapping = $state(false);
  let customTest = $state<{ running: boolean; error: string | null; response: unknown; preview: CustomMappingPreview | null; url: string | null }>({
    running: false,
    error: null,
    response: null,
    preview: null,
    url: null,
  });
  let peerSyncPending = $state<Record<string, boolean>>({});

  const peers = peersStore.peers;
  const quickSetupEnabled = $derived(isQuickSetupKind(customForm.integrationKind));
  let viewFilter = $state<PeerFilterOption>('all');
  let searchQuery = $state('');
  const filteredPeers = $derived.by<PeerSummary[]>(() => {
    const list = $peers ?? [];
    const baseList = viewFilter === 'all' ? list : list.filter((peer) => peer.integration.kind === viewFilter);
    const query = searchQuery.trim().toLowerCase();
    if (!query) return baseList;
    return baseList.filter((peer) => peerSearchableContent(peer).includes(query));
  });
  const discovery = peersStore.discovery;
  const fetchedAt = peersStore.fetchedAt;
  let discoverModalOpen = $state(false);
  type ModalView = 'discover' | 'helios' | 'limelight_os' | 'photonvision' | 'manual';
  let modalView = $state<ModalView>('discover');

  const isInitialLoading = $derived(!$hasLoadedOnce && $pending.refresh);

  onMount(() => {
    void refreshPeers({ bootstrap: true });
  });

  $effect(() => {
    if (!quickSetupEnabled) {
      lastPresetKey = null;
      return;
    }
    const host = normalizeDeviceHost(customForm.deviceIp);
    if (!host) {
      lastPresetKey = null;
      return;
    }
    const key = `${customForm.integrationKind}:${host}`;
    if (key === lastPresetKey) return;
    applyPreset(customForm.integrationKind, host);
    lastPresetKey = key;
  });

  $effect(() => {
    if (customForm.integrationKind !== 'photonvision') {
      photonvisionState = { host: null, pending: false, error: null, streams: [] };
      if (customForm.streamUrls.length > 0) {
        customForm = { ...customForm, streamUrls: [] };
      }
      return;
    }
    const host = normalizeDeviceHost(customForm.deviceIp);
    if (!host) {
      photonvisionState = { host: null, pending: false, error: null, streams: [] };
      return;
    }
    if (photonvisionState.pending) return;
    if (photonvisionState.host === host && photonvisionState.streams.length > 0) return;
    void refreshPhotonvisionStreams(host);
  });

  async function refreshPhotonvisionStreams(host: string): Promise<void> {
    if (!host.trim() || photonvisionState.pending) return;
    photonvisionState = { ...photonvisionState, pending: true, error: null, host };
    try {
      const response = await discoverPhotonvisionStreams(host, 2500);
      const streams = response.streams ?? [];
      photonvisionState = { host, pending: false, error: null, streams };
      const urls = streams.map((stream) => stream.url);
      customForm = { ...customForm, streamUrls: urls };
      if (urls.length > 0) {
        const current = customForm.streamUrl.trim();
        if (!current || !urls.includes(current)) {
          customForm = { ...customForm, streamUrl: urls[0] };
        }
      }
    } catch (error) {
      const message = buildErrorMessage({ error, fallback: 'Unable to discover PhotonVision streams.' });
      photonvisionState = { ...photonvisionState, pending: false, error: message, streams: [] };
    }
  }

  async function refreshPeers(options: { bootstrap?: boolean } = {}): Promise<void> {
    const { bootstrap = false } = options;
    if ($pending.refresh) return;
    if (bootstrap) {
      loadError = null;
    }
    try {
      const payload = await peersStore.refresh({ bootstrap });
      loadError = payload.errorMessage ?? null;
    } catch (error) {
      console.error('Failed to refresh peers', error);
      loadError = reportError({ title: 'Peer refresh failed', error, inline: (message) => (loadError = message) });
    }
  }

  async function handleDiscover(): Promise<void> {
    if ($pending.discover) return;
    try {
      const info = await peersStore.discover({ scopes: ['mdns', 'broadcast'], timeoutSecs: 5 });
      toaster.success({ title: 'Discovery complete', description: `Run ${info.runId}` });
    } catch (error) {
      reportError({ title: 'Discovery failed', error });
    }
  }

  async function handleRemove(peerId: string): Promise<void> {
    if (!peerId || $pending.removing.includes(peerId)) return;
    try {
      const removed = await peersStore.removePeerById(peerId);
      if (removed) {
        toaster.success({ title: 'Peer removed', description: peerId });
      } else {
        toaster.error({ title: 'Remove failed', description: 'Peer not found.' });
      }
    } catch (error) {
      reportError({ title: 'Remove failed', error });
    }
  }

  function isSyncingPeer(peerId: string): boolean {
    return Boolean(peerSyncPending[peerId]);
  }

  function setSyncingPeer(peerId: string, value: boolean): void {
    peerSyncPending = { ...peerSyncPending, [peerId]: value };
  }

  async function handleSyncPeerPipelines(peer: PeerSummary): Promise<void> {
    if (!peer?.id || isSyncingPeer(peer.id)) return;
    setSyncingPeer(peer.id, true);
    try {
      const response = await syncPeerPipelines(peer.id, { force: true }, 10_000);
      const syncedCount = response.synced.length;
      const errorCount = response.errors.length;
      if (errorCount > 0) {
        toaster.warning({
          title: 'Peer sync completed with warnings',
          description: `${displayName(peer)} · ${syncedCount} synced · ${errorCount} warning(s)`
        });
      } else {
        toaster.success({
          title: 'Peer pipelines synced',
          description: `${displayName(peer)} · ${syncedCount} pipeline(s)`
        });
      }
    } catch (error) {
      reportError({ title: 'Pipeline sync failed', error });
    } finally {
      setSyncingPeer(peer.id, false);
    }
  }

  function applyPreset(kind: PeerIntegrationKind, host: string): void {
    const preset = presetFor(kind, host);
    customForm = {
      ...customForm,
      apiBaseUrl: preset.apiBaseUrl,
      managementUrl: preset.managementUrl,
      streamUrl: preset.streamUrl,
      streamUrls: preset.streamUrl ? [preset.streamUrl] : [],
      endpointHost: preset.endpointHost,
      endpointPort: preset.endpointPort,
    };
    showManagement = true;
  }

  function resetCustomForm(): void {
    customForm = {
      peerId: null,
      alias: '',
      deviceIp: '',
      apiBaseUrl: '',
      endpointHost: '',
      endpointPort: '',
      managementUrl: '',
      streamUrl: '',
      streamUrls: [],
      apiEndpoint: '',
      networkTable: '',
      telemetryEndpoint: '',
      integrationKind: 'custom',
      integrationBaseline: defaultIntegration(),
      mapping: blankMappingForm(),
      poseSource: 'http',
      arucoSource: 'http',
    };
    lastPresetKey = null;
    photonvisionState = { host: null, pending: false, error: null, streams: [] };
    showManagement = false;
    showPoseMapping = false;
    showArucoMapping = false;
    customTest = { running: false, error: null, response: null, preview: null, url: null };
  }

  function populateCustomFormFromPeer(peer?: PeerSummary | null): void {
    resetCustomForm();
    if (!peer) return;
    const endpoints = peer.endpoints ?? [];
    const primary = endpoints[0];
    const integration = cloneIntegrationMetadata(peer.integration);
    const custom = integration.custom;
    const streamUrls = Array.isArray(integration.streamUrls) && integration.streamUrls.length > 0 ? integration.streamUrls : integration.streamUrl ? [integration.streamUrl] : [];
    customForm = {
      peerId: peer.id,
      alias: peer.alias ?? '',
      deviceIp: deviceHostFromApiBase(peer.apiBaseUrl ?? ''),
      apiBaseUrl: peer.apiBaseUrl ?? '',
      endpointHost: primary?.host ?? '',
      endpointPort: typeof primary?.port === 'number' && Number.isFinite(primary.port) ? String(primary.port) : '',
      managementUrl: integration.managementUrl ?? '',
      streamUrl: integration.streamUrl ?? '',
      streamUrls,
      apiEndpoint: custom?.apiEndpoint ?? '',
      networkTable: custom?.networkTable ?? '',
      telemetryEndpoint: custom?.telemetryEndpoint ?? '',
      integrationKind: integration.kind,
      integrationBaseline: integration,
      mapping: mappingFormFromIntegration(custom?.mapping),
      poseSource: 'http',
      arucoSource: 'http',
    };
    const host = normalizeDeviceHost(customForm.deviceIp);
    lastPresetKey = host ? `${customForm.integrationKind}:${host}` : null;
    showManagement = Boolean(integration.managementUrl || integration.streamUrl || customForm.endpointHost || customForm.endpointPort);
    const mapping = custom?.mapping;
    showPoseMapping = Boolean(mapping?.pose);
    showArucoMapping = Boolean(mapping?.aruco);
  }

  async function testPeerConnection(): Promise<void> {
    if (customPending.probe) return;
    const deviceHost = normalizeDeviceHost(customForm.deviceIp);
    customPending = { ...customPending, probe: true };
    try {
      const response = await probePeer(
        {
          kind: customForm.integrationKind,
          apiBaseUrl: customForm.apiBaseUrl,
          managementUrl: customForm.managementUrl,
          deviceIp: deviceHost || null,
          streamUrl: customForm.streamUrl,
          streamUrls: customForm.streamUrls,
          networkTable: customForm.networkTable,
          timeoutMs: 2000,
        },
        5000
      );
      applyProbeResponse(response);
      const apiOk = response.api?.ok ?? false;
      const mgmtOk = response.management?.ok ?? false;
      const streamOkCount = (response.streams ?? []).filter((item) => item.ok).length;
      const streamTotal = (response.streams ?? []).length;
      toaster.success({
        title: 'Probe complete',
        description: `API ${apiOk ? 'ok' : 'fail'} · UI ${mgmtOk ? 'ok' : 'fail'} · Streams ${streamOkCount}/${streamTotal}`,
      });
      if (customForm.peerId) {
        peerProbeCache = { ...peerProbeCache, [customForm.peerId]: response };
      }
    } catch (error) {
      reportError({ title: 'Probe failed', error });
    } finally {
      customPending = { ...customPending, probe: false };
    }
  }

  function applyProbeResponse(response: PeerProbeResponse): void {
    if (response.kind !== 'photonvision') return;
    const discovered = response.photonvision?.streams ?? [];
    if (discovered.length === 0) return;
    const urls = discovered.map((stream) => stream.url);
    photonvisionState = { host: response.photonvision?.host ?? photonvisionState.host, pending: false, error: null, streams: discovered };
    customForm = { ...customForm, streamUrls: urls };
    const current = customForm.streamUrl.trim();
    if (!current || !urls.includes(current)) {
      customForm = { ...customForm, streamUrl: urls[0] };
    }
  }


  async function probeExistingPeer(peer: PeerSummary): Promise<void> {
    if (customPending.probe) return;
    customPending = { ...customPending, probe: true };
    try {
      const response = await probePeer(
        {
          kind: peer.integration.kind,
          apiBaseUrl: peer.apiBaseUrl,
          managementUrl: peer.integration.managementUrl,
          deviceIp: normalizeDeviceHost(peer.apiBaseUrl),
          streamUrl: peer.integration.streamUrl,
          streamUrls: peer.integration.streamUrls,
          networkTable: peer.integration.custom?.networkTable ?? null,
          timeoutMs: 2000,
        },
        5000
      );
      peerProbeCache = { ...peerProbeCache, [peer.id]: response };
      const apiOk = response.api?.ok ?? false;
      const mgmtOk = response.management?.ok ?? false;
      const streamOkCount = (response.streams ?? []).filter((item) => item.ok).length;
      const streamTotal = (response.streams ?? []).length;
      toaster.success({ title: 'Probe complete', description: `${displayName(peer)} · API ${apiOk ? 'ok' : 'fail'} · UI ${mgmtOk ? 'ok' : 'fail'} · Streams ${streamOkCount}/${streamTotal}` });
    } catch (error) {
      reportError({ title: 'Probe failed', error });
    } finally {
      customPending = { ...customPending, probe: false };
    }
  }

  async function handleCustomSave(event?: SubmitEvent): Promise<void> {
    event?.preventDefault();
    if (customPending.customSave) return;
    const quickSetup = isQuickSetupKind(customForm.integrationKind);
    const deviceHost = normalizeDeviceHost(customForm.deviceIp);
    if (quickSetup && !deviceHost) {
      toaster.error({ title: 'Save failed', description: 'Device IP is required for this integration.' });
      return;
    }
    if (!customForm.apiBaseUrl.trim() && quickSetup && deviceHost) {
      applyPreset(customForm.integrationKind, deviceHost);
    }
    const apiBaseUrl = customForm.apiBaseUrl.trim();
    if (!apiBaseUrl) {
      toaster.error({ title: 'Save failed', description: 'API base URL is required.' });
      return;
    }
    customPending = { ...customPending, customSave: true };
    try {
      const integration = composeIntegrationFromForm({
        integrationBaseline: customForm.integrationBaseline,
        integrationKind: customForm.integrationKind,
        managementUrl: customForm.managementUrl,
        streamUrl: customForm.streamUrl,
        streamUrls: customForm.streamUrls,
        poseSource: customForm.poseSource,
        arucoSource: customForm.arucoSource,
        apiEndpoint: customForm.apiEndpoint,
        networkTable: customForm.networkTable,
        telemetryEndpoint: customForm.telemetryEndpoint,
        mapping: customForm.mapping
      });
      const peer = await registerPeer({
        peerId: customForm.peerId,
        alias: customForm.alias.trim() || null,
        apiBaseUrl,
        deviceIp: deviceHost || null,
        endpointHost: customForm.endpointHost.trim() || null,
        endpointPort: parsePort(customForm.endpointPort),
        integration,
      });
      upsertPeer(peer);
      toaster.success({
        title: customForm.peerId ? 'Peer updated' : 'Custom peer queued',
        description: displayName(peer),
      });
      discoverModalOpen = false;
    } catch (error) {
      reportError({ title: 'Save failed', error });
    } finally {
      customPending = { ...customPending, customSave: false };
    }
  }

  async function testCustomEndpoint(): Promise<void> {
    if (customPending.customTest) return;
    if (customForm.poseSource === 'nt' && customForm.arucoSource === 'nt') {
      customTest = { ...customTest, error: 'Endpoint testing uses HTTP; switch one mapping to HTTP or provide an API endpoint.', preview: null, response: null, url: null };
      return;
    }
    const endpoint = customForm.apiEndpoint.trim();
    if (!endpoint.length) {
      customTest = { ...customTest, error: 'API endpoint is required for testing.', preview: null, response: null, url: null };
      return;
    }
    const base = customForm.apiBaseUrl.trim();
    customPending = { ...customPending, customTest: true };
    customTest = { ...customTest, running: true, error: null, preview: null, response: null };
    try {
      const target = buildCustomUrl(base, endpoint);
      const response = await fetch(target, { headers: { accept: 'application/json' } });
      if (!response.ok) {
        const detail = await response.text().catch(() => '');
        throw new Error(detail || `Request failed (${response.status})`);
      }
      const payload = await response.json();
      const preview = previewCustomMapping(payload, mappingFromForm(customForm.mapping));
      customTest = { running: false, error: null, response: payload, preview, url: target };
    } catch (error) {
      const message = buildErrorMessage({ error, fallback: 'Unable to test the endpoint right now.' });
      customTest = { running: false, error: message, response: null, preview: null, url: null };
    } finally {
      customPending = { ...customPending, customTest: false };
    }
  }

  function upsertPeer(peer: PeerSummary): void {
    peersStore.upsertPeer(peer);
  }

  function openManagementUi(peer: PeerSummary): void {
    const target = managementTarget(peer);
    if (!target) return;
    if (typeof window !== 'undefined') {
      window.open(target, '_blank', 'noopener,noreferrer');
    }
  }

  function isRemoving(peerId: string): boolean {
    return $pending.removing.includes(peerId);
  }

  function selectModalView(view: ModalView, peer?: PeerSummary | null): void {
    if (view === 'manual') {
      populateCustomFormFromPeer(peer ?? null);
      customForm.integrationKind = 'custom';
    } else if (view !== 'discover') {
      customForm.integrationKind = view;
    }
    modalView = view;
  }

  function openDiscoverModal(view: ModalView = 'discover', peer?: PeerSummary | null): void {
    selectModalView(view, peer ?? null);
    discoverModalOpen = true;
  }

  function closeDiscoverModal(): void {
    discoverModalOpen = false;
    resetCustomForm();
  }

  function handlePeerFilterChange(filterId: PeerFilterOption): void {
    viewFilter = filterId;
  }


  const lastFetchedLabel = $derived.by(
    () =>
      new Intl.DateTimeFormat(undefined, {
        dateStyle: 'short',
        timeStyle: 'medium',
      }).format($fetchedAt ?? Date.now()),
  );
</script>

<section class="flex min-h-0 flex-1 flex-col gap-6">

  {#if loadError}
    <div class="rounded border border-amber-600 bg-amber-600/20 px-4 py-3 text-sm text-amber-100">
      {loadError}
    </div>
  {/if}

  <div class="flex flex-1 flex-col gap-6 lg:min-h-0 lg:flex-row">
    <PeersSidebar
      bind:searchQuery
      bind:viewFilter
      filters={FILTERS}
      onAddPeer={() => openDiscoverModal('discover')}
      onFilterChange={(id) => handlePeerFilterChange(id)}
    />

    <div class="flex min-h-0 flex-1 flex-col gap-6">
      <PeersListPanel
        {isInitialLoading}
        peers={$peers}
        {filteredPeers}
        viewFilter={viewFilter}
        filters={FILTERS}
        {searchQuery}
        {peerProbeCache}
        {customPending}
        onSyncPipelines={(peer) => handleSyncPeerPipelines(peer)}
        onOpenMapping={(peer) => openDiscoverModal('manual', peer)}
        onProbePeer={(peer) => probeExistingPeer(peer)}
        onOpenManagement={(peer) => openManagementUi(peer)}
        onRemove={(peerId) => handleRemove(peerId)}
        isRemoving={(peerId) => isRemoving(peerId)}
        isSyncingPipelines={(peerId) => isSyncingPeer(peerId)}
      />
    </div>
  </div>
</section>

{#if discoverModalOpen}
  <PeersDiscoverModal
    peers={$peers}
    modalView={modalView}
    customForm={customForm}
    customPending={customPending}
    customTest={customTest}
    photonvisionState={photonvisionState}
    discovery={$discovery}
    {lastFetchedLabel}
    discoverPending={$pending.discover}
    bind:showManagement
    bind:showPoseMapping
    bind:showArucoMapping
    onClose={closeDiscoverModal}
    onSelectView={(view, peer) => selectModalView(view, peer)}
    onDiscover={handleDiscover}
    onSave={handleCustomSave}
    onTestConnection={testPeerConnection}
    onTestEndpoint={testCustomEndpoint}
    onRefreshPhotonvisionStreams={(host) => refreshPhotonvisionStreams(host)}
  />
{/if}
