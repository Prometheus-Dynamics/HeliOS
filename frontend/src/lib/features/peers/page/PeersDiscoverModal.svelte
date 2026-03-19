<script lang="ts">
  import { faTowerBroadcast, faWrench } from '@fortawesome/free-solid-svg-icons';
  import FaIcon from '$lib/components/icons/FaIcon.svelte';
  import type { CustomMappingForm, CustomMappingPreview } from '$lib/features/peers/types';
  import { formatDiscovery, integrationLabel, normalizeDeviceHost } from '$lib/features/peers/utils';
  import type {
    DiscoveredStream,
    PeerDiscoveryInfo,
    PeerIntegrationKind,
    PeerSummary
  } from '$lib/types/peer';

  type ModalView = 'discover' | 'helios' | 'limelight_os' | 'photonvision' | 'manual';

  type CustomPending = {
    customSave: boolean;
    customTest: boolean;
    probe: boolean;
  };

  type CustomTest = {
    error: string | null;
    response: unknown;
    preview: CustomMappingPreview | null;
    url: string | null;
  };

  type CustomPeerForm = {
    peerId: string | null;
    alias: string;
    deviceIp: string;
    apiBaseUrl: string;
    endpointHost: string;
    endpointPort: string;
    managementUrl: string;
    streamUrl: string;
    streamUrls: string[];
    apiEndpoint: string;
    networkTable: string;
    telemetryEndpoint: string;
    integrationKind: PeerIntegrationKind;
    mapping: CustomMappingForm;
    poseSource: 'http' | 'nt';
    arucoSource: 'http' | 'nt';
  };

  type PhotonvisionState = {
    host: string | null;
    pending: boolean;
    error: string | null;
    streams: DiscoveredStream[];
  };

  type Props = {
    peers: PeerSummary[];
    modalView: ModalView;
    customForm: CustomPeerForm;
    customPending: CustomPending;
    customTest: CustomTest;
    photonvisionState: PhotonvisionState;
    discovery: PeerDiscoveryInfo | null;
    lastFetchedLabel: string;
    discoverPending: boolean;
    showManagement: boolean;
    showPoseMapping: boolean;
    showArucoMapping: boolean;
    onClose: () => void;
    onSelectView: (view: ModalView, peer?: PeerSummary | null) => void;
    onDiscover: () => void;
    onSave: (event?: SubmitEvent) => void;
    onTestConnection: () => void;
    onTestEndpoint: () => void;
    onRefreshPhotonvisionStreams: (host: string) => void;
  };

  const INPUT_CLASSES =
    'rounded border border-surface-700 bg-surface-900 px-3 py-2 text-sm text-surface-100 focus:border-primary-500 focus:outline-none';

  let {
    peers,
    modalView,
    customForm,
    customPending,
    customTest,
    photonvisionState,
    discovery,
    lastFetchedLabel,
    discoverPending,
    showManagement = $bindable(),
    showPoseMapping = $bindable(),
    showArucoMapping = $bindable(),
    onClose,
    onSelectView,
    onDiscover,
    onSave,
    onTestConnection,
    onTestEndpoint,
    onRefreshPhotonvisionStreams
  }: Props = $props();

  const modalHeading = (view: ModalView): string => {
    if (view === 'discover') return 'Add peer';
    if (view === 'manual') return customForm.peerId ? 'Edit peer mapping' : 'Manual peer';
    if (view === 'helios') return 'HeliOS peer';
    if (view === 'limelight_os') return 'LimelightOS peer';
    return 'PhotonVision peer';
  };

  const modalSubheading = (view: ModalView): string => {
    if (view === 'discover') return 'Discover devices or add one manually.';
    if (view === 'manual') return 'Full control over endpoints, mappings, and telemetry.';
    if (view === 'helios') return 'Connect a Helios node by IP and we fill in the rest.';
    if (view === 'limelight_os') return 'Provide the device IP to pull the management and stream endpoints.';
    return 'Provide the device IP and the PhotonVision defaults will be applied.';
  };

  const modalTitle = $derived(modalHeading(modalView));
  const modalSubtitle = $derived(modalSubheading(modalView));
</script>

<div class="fixed inset-0 z-50 flex items-center justify-center" role="presentation">
  <button
    type="button"
    class="absolute inset-0 bg-black/70"
    aria-label="Close discover peers"
    onclick={onClose}
  ></button>
  <div
    class="relative z-10 w-full max-w-6xl rounded-lg border border-surface-700 bg-surface-950 text-surface-100 shadow-2xl"
    role="dialog"
    aria-modal="true"
    aria-labelledby="discover-peers-title"
    tabindex="-1"
  >
    <header class="flex items-start justify-between border-b border-surface-800 px-6 py-4">
      <div>
        <p class="text-xs uppercase tracking-[0.4em] text-surface-500">Cluster</p>
        <h2 id="discover-peers-title" class="text-xl font-semibold text-surface-50">{modalTitle}</h2>
        <p class="text-sm text-surface-400">{modalSubtitle}</p>
      </div>
      <button class="rounded border border-surface-700 px-2 py-1 text-sm text-surface-400 transition hover:text-surface-100" type="button" onclick={onClose}>
        Close
      </button>
    </header>
    <div class="flex flex-wrap gap-2 border-b border-surface-800 px-6 py-3">
      <button
        class={`btn btn-2xs ${modalView === 'discover' ? 'preset-filled-primary-500' : 'preset-tonal'}`}
        type="button"
        onclick={() => onSelectView('discover')}
      >
        <span class="inline-flex items-center gap-2">
          <FaIcon icon={faTowerBroadcast} class="h-3 w-3" />
          Auto discover
        </span>
      </button>
      <button
        class={`btn btn-2xs ${modalView === 'helios' ? 'preset-filled-primary-500' : 'preset-tonal'}`}
        type="button"
        onclick={() => onSelectView('helios')}
      >
        <span class="inline-flex items-center gap-2">
          <img src="/logo.svg" alt="" class="h-3.5 w-3.5" />
          HeliOS
        </span>
      </button>
      <button
        class={`btn btn-2xs ${modalView === 'limelight_os' ? 'preset-filled-primary-500' : 'preset-tonal'}`}
        type="button"
        onclick={() => onSelectView('limelight_os')}
      >
        <span class="inline-flex items-center gap-2">
          <img src="/logos/limelight_icon.webp" alt="" class="h-3.5 w-3.5" />
          LimelightOS
        </span>
      </button>
      <button
        class={`btn btn-2xs ${modalView === 'photonvision' ? 'preset-filled-primary-500' : 'preset-tonal'}`}
        type="button"
        onclick={() => onSelectView('photonvision')}
      >
        <span class="inline-flex items-center gap-2">
          <img src="/logos/photonvision_logo.png" alt="" class="h-3.5 w-3.5" />
          PhotonVision
        </span>
      </button>
      <button
        class={`btn btn-2xs ${modalView === 'manual' ? 'preset-filled-primary-500' : 'preset-tonal'}`}
        type="button"
        onclick={() =>
          onSelectView(
            'manual',
            customForm.peerId ? peers.find((peer) => peer.id === customForm.peerId) ?? null : null
          )}
      >
        <span class="inline-flex items-center gap-2">
          <FaIcon icon={faWrench} class="h-3 w-3" />
          Manual
        </span>
      </button>
    </div>
    <div class="px-6 py-5">
      {#if modalView === 'discover'}
        <div class="space-y-4 text-sm text-surface-300">
          <p>
            Kick off a discovery scan to find LimelightOS, PhotonVision, or Helios peers on your network. Discovery fans
            out over mDNS and broadcast scopes and automatically registers any peers that respond.
          </p>
          <div class="flex flex-wrap items-center gap-3">
            <button class="btn btn-sm preset-filled-primary-500" type="button" onclick={onDiscover} disabled={discoverPending}>
              {discoverPending ? 'Discovering…' : 'Start discovery'}
            </button>
            <button class="btn btn-sm preset-tonal" type="button" onclick={() => onSelectView('manual')}>
              Add manually instead
            </button>
          </div>
          <div class="rounded border border-surface-800 bg-surface-900/40 p-3 text-xs text-surface-400">
            <p class="font-semibold text-surface-200">Recent activity</p>
            <p class="mt-1">{formatDiscovery(discovery)}</p>
            <p class="mt-2">Last refreshed {lastFetchedLabel}</p>
          </div>
        </div>
      {:else}
        <form class="space-y-4 text-sm text-surface-300" onsubmit={onSave}>
          <div class="grid gap-4 lg:grid-cols-3">
            <div class="space-y-4 lg:col-span-2">
              <section class="space-y-3 rounded border border-surface-800 bg-surface-950/50 p-4">
                <div class="flex items-start justify-between gap-3">
                  <div>
                    <p class="text-micro uppercase tracking-[0.3em] text-surface-500">Setup</p>
                    <p class="text-xs text-surface-400">Add the device and let Helios configure the defaults.</p>
                  </div>
                  <span class="rounded border border-surface-700/60 px-2 py-1 text-micro-tight uppercase tracking-[0.3em] text-surface-300">
                    {modalView === 'manual' ? 'Custom' : integrationLabel(customForm.integrationKind)}
                  </span>
                </div>
                <div class="grid gap-3 sm:grid-cols-2">
                  <label class="flex flex-col gap-1 text-sm text-surface-200">
                    Alias (optional)
                    <input class={INPUT_CLASSES} bind:value={customForm.alias} placeholder="Front camera" />
                  </label>
                  {#if modalView === 'manual'}
                    <label class="flex flex-col gap-1 text-sm text-surface-200">
                      API base URL
                      <input class={INPUT_CLASSES} bind:value={customForm.apiBaseUrl} placeholder="http://10.0.0.30:5800" required />
                      <p class="text-[0.7rem] text-surface-500">Primary endpoint for pose + detections.</p>
                    </label>
                  {:else}
                    <label class="flex flex-col gap-1 text-sm text-surface-200">
                      Device IP / host
                      <input class={INPUT_CLASSES} bind:value={customForm.deviceIp} placeholder="192.168.0.14" required />
                      <p class="text-[0.7rem] text-surface-500">Auto-fills API base, stream URL, and management UI.</p>
                    </label>
                  {/if}
                </div>
                {#if modalView === 'manual'}
                  <div class="grid gap-3 sm:grid-cols-2">
                    <label class="flex flex-col gap-1 text-sm text-surface-200">
                      Data API endpoint (optional)
                      <input class={INPUT_CLASSES} bind:value={customForm.apiEndpoint} placeholder="/v1/localization" />
                      <p class="text-[0.7rem] text-surface-500">Single endpoint for pose + detections.</p>
                    </label>
                    <label class="flex flex-col gap-1 text-sm text-surface-200">
                      NetworkTables path (optional)
                      <input class={INPUT_CLASSES} bind:value={customForm.networkTable} placeholder="/Vision/Results" />
                      <p class="text-[0.7rem] text-surface-500">Use NT instead of HTTP for pose/detections.</p>
                    </label>
                    <label class="flex flex-col gap-1 text-sm text-surface-200 sm:col-span-2">
                      Telemetry endpoint (optional)
                      <input class={INPUT_CLASSES} bind:value={customForm.telemetryEndpoint} placeholder="/v1/telemetry/resource" />
                      <p class="text-[0.7rem] text-surface-500">Overrides the API base URL for CPU/memory telemetry.</p>
                    </label>
                  </div>
                {/if}
              </section>

              <section class="space-y-3 rounded border border-surface-800 bg-surface-950/50 p-4">
                <div class="flex items-start justify-between gap-3">
                  <div>
                    <p class="text-micro uppercase tracking-[0.3em] text-surface-500">Endpoints</p>
                    <p class="text-xs text-surface-500">We will register these URLs for the peer.</p>
                  </div>
                  <button class="btn btn-2xs preset-tonal" type="button" onclick={() => (showManagement = !showManagement)}>
                    {showManagement ? 'Hide advanced' : 'Edit endpoints'}
                  </button>
                </div>
                <div class="grid gap-3 sm:grid-cols-2 text-xs">
                  <div class="rounded border border-surface-800/70 bg-surface-950/60 p-3">
                    <p class="text-micro-tight uppercase tracking-[0.3em] text-surface-500">API base</p>
                    <p class="mt-1 text-surface-200">{customForm.apiBaseUrl || 'Auto-filled'}</p>
                  </div>
                  <div class="rounded border border-surface-800/70 bg-surface-950/60 p-3">
                    <p class="text-micro-tight uppercase tracking-[0.3em] text-surface-500">Management UI</p>
                    <p class="mt-1 text-surface-200">{customForm.managementUrl || 'Auto-filled'}</p>
                  </div>
                  <div class="rounded border border-surface-800/70 bg-surface-950/60 p-3">
                    <p class="text-micro-tight uppercase tracking-[0.3em] text-surface-500">Stream URL</p>
                    <p class="mt-1 text-surface-200">
                      {customForm.streamUrl || 'Auto-filled'}
                      {#if customForm.integrationKind === 'photonvision' && customForm.streamUrls.length > 1}
                        <span class="ml-2 text-[0.7rem] text-surface-500">(+{customForm.streamUrls.length - 1} more)</span>
                      {/if}
                    </p>
                  </div>
                  <div class="rounded border border-surface-800/70 bg-surface-950/60 p-3">
                    <p class="text-micro-tight uppercase tracking-[0.3em] text-surface-500">Endpoint</p>
                    <p class="mt-1 text-surface-200">
                      {customForm.endpointHost || 'Auto-filled'}{customForm.endpointPort ? `:${customForm.endpointPort}` : ''}
                    </p>
                  </div>
                </div>
                {#if showManagement}
                  <div class="grid gap-3 sm:grid-cols-2">
                    <label class="flex flex-col gap-1 text-sm text-surface-200 sm:col-span-2">
                      API base URL
                      <input class={INPUT_CLASSES} bind:value={customForm.apiBaseUrl} placeholder="http://10.0.0.30:5800" />
                    </label>
                    <label class="flex flex-col gap-1 text-sm text-surface-200">
                      Management UI
                      <input class={INPUT_CLASSES} bind:value={customForm.managementUrl} placeholder="http://10.0.0.30:5801" />
                    </label>
                    <label class="flex flex-col gap-1 text-sm text-surface-200">
                      Stream URL
                      <input class={INPUT_CLASSES} bind:value={customForm.streamUrl} placeholder="rtsp://10.0.0.30:8554/main" />
                      {#if customForm.integrationKind === 'photonvision'}
                        <div class="mt-1 flex flex-wrap items-center gap-2 text-[0.7rem] text-surface-500">
                          <p>PhotonVision streams often live on `:1181`, `:1182`, …</p>
                          <button
                            class="btn btn-2xs preset-tonal"
                            type="button"
                            onclick={() => onRefreshPhotonvisionStreams(normalizeDeviceHost(customForm.deviceIp) ?? '')}
                            disabled={photonvisionState.pending}
                          >
                            {photonvisionState.pending ? 'Scanning…' : 'Scan streams'}
                          </button>
                        </div>
                        {#if photonvisionState.error}
                          <p class="text-[0.7rem] text-red-300">{photonvisionState.error}</p>
                        {/if}
                        {#if photonvisionState.streams.length > 0}
                          <label class="mt-2 flex flex-col gap-1 text-sm text-surface-200">
                            Select stream
                            <select class={INPUT_CLASSES} bind:value={customForm.streamUrl}>
                              {#each photonvisionState.streams as stream (stream.url)}
                                <option value={stream.url}>{stream.url}</option>
                              {/each}
                            </select>
                            <p class="text-[0.7rem] text-surface-500">
                              Discovered {photonvisionState.streams.length} stream(s) on {photonvisionState.host}.
                            </p>
                          </label>
                          <p class="mt-2 text-[0.7rem] text-surface-500">
                            For best results, set the peer camera stream encoder to a high output resolution and bitrate
                            (MJPEG).
                          </p>
                        {/if}
                      {/if}
                    </label>
                    <label class="flex flex-col gap-1 text-sm text-surface-200">
                      Endpoint host
                      <input class={INPUT_CLASSES} bind:value={customForm.endpointHost} placeholder="10.0.0.30" />
                    </label>
                    <label class="flex flex-col gap-1 text-sm text-surface-200">
                      Endpoint port
                      <input class={INPUT_CLASSES} bind:value={customForm.endpointPort} type="number" min="1" max="65535" placeholder="5800" />
                    </label>
                  </div>
                {/if}
              </section>

              {#if modalView === 'manual'}
                <div class="flex flex-wrap gap-2 text-xs">
                  <button class="btn btn-2xs preset-tonal" type="button" onclick={() => (showPoseMapping = !showPoseMapping)}>
                    {showPoseMapping ? 'Hide pose' : 'Add pose mapping'}
                  </button>
                  <button class="btn btn-2xs preset-tonal" type="button" onclick={() => (showArucoMapping = !showArucoMapping)}>
                    {showArucoMapping ? 'Hide ArUco tags' : 'Add ArUco tags'}
                  </button>
                </div>

                <div class="grid gap-3 auto-rows-max lg:grid-cols-2">
                  {#if showPoseMapping}
                    <section class="space-y-2 rounded border border-surface-800 bg-surface-950/50 p-3">
                      <div class="flex items-center justify-between">
                        <div>
                          <p class="text-micro uppercase tracking-[0.3em] text-surface-500">Localization</p>
                          <p class="text-xs text-surface-500">JSON paths for robot pose from API/NT.</p>
                        </div>
                        <div class="flex items-center gap-1 text-[0.75rem]">
                          <span class="text-surface-500">Source</span>
                          <div class="flex overflow-hidden rounded border border-surface-700">
                            <button
                              type="button"
                              class={`px-2 py-1 ${customForm.poseSource === 'http' ? 'bg-primary-600/30 text-primary-100' : 'bg-surface-900 text-surface-300'}`}
                              onclick={() => (customForm.poseSource = 'http')}
                            >
                              HTTP
                            </button>
                            <button
                              type="button"
                              class={`px-2 py-1 ${customForm.poseSource === 'nt' ? 'bg-primary-600/30 text-primary-100' : 'bg-surface-900 text-surface-300'}`}
                              onclick={() => (customForm.poseSource = 'nt')}
                            >
                              NT
                            </button>
                          </div>
                        </div>
                      </div>
                      <div class="grid gap-2 sm:grid-cols-3">
                        <label class="flex flex-col gap-1 text-surface-200">
                          X (m)
                          <input class={INPUT_CLASSES} bind:value={customForm.mapping.poseTranslationX} placeholder="pose.translation.x" />
                        </label>
                        <label class="flex flex-col gap-1 text-surface-200">
                          Y (m)
                          <input class={INPUT_CLASSES} bind:value={customForm.mapping.poseTranslationY} placeholder="pose.translation.y" />
                        </label>
                        <label class="flex flex-col gap-1 text-surface-200">
                          Z (m)
                          <input class={INPUT_CLASSES} bind:value={customForm.mapping.poseTranslationZ} placeholder="pose.translation.z" />
                        </label>
                      </div>
                      <div class="grid gap-2 sm:grid-cols-3">
                        <label class="flex flex-col gap-1 text-surface-200">
                          Roll (°)
                          <input class={INPUT_CLASSES} bind:value={customForm.mapping.poseRotationRoll} placeholder="pose.rotation.roll" />
                        </label>
                        <label class="flex flex-col gap-1 text-surface-200">
                          Pitch (°)
                          <input class={INPUT_CLASSES} bind:value={customForm.mapping.poseRotationPitch} placeholder="pose.rotation.pitch" />
                        </label>
                        <label class="flex flex-col gap-1 text-surface-200">
                          Yaw (°)
                          <input class={INPUT_CLASSES} bind:value={customForm.mapping.poseRotationYaw} placeholder="pose.rotation.yaw" />
                        </label>
                      </div>
                      <div class="grid gap-2 sm:grid-cols-2">
                        <label class="flex flex-col gap-1 text-surface-200">
                          Timestamp path
                          <input class={INPUT_CLASSES} bind:value={customForm.mapping.poseTimestamp} placeholder="pose.timestamp" />
                        </label>
                        <label class="flex flex-col gap-1 text-surface-200">
                          Latency (ms) path
                          <input class={INPUT_CLASSES} bind:value={customForm.mapping.poseLatencyMs} placeholder="pose.latency_ms" />
                        </label>
                      </div>
                    </section>
                  {/if}

                  {#if showArucoMapping}
                    <section class="space-y-2 rounded border border-surface-800 bg-surface-950/50 p-3">
                      <div class="flex items-center justify-between">
                        <div>
                          <p class="text-micro uppercase tracking-[0.3em] text-surface-500">ArUco mapping</p>
                          <p class="text-xs text-surface-500">Point Helios to detections fields.</p>
                        </div>
                        <div class="flex items-center gap-1 text-[0.75rem]">
                          <span class="text-surface-500">Source</span>
                          <div class="flex overflow-hidden rounded border border-surface-700">
                            <button
                              type="button"
                              class={`px-2 py-1 ${customForm.arucoSource === 'http' ? 'bg-primary-600/30 text-primary-100' : 'bg-surface-900 text-surface-300'}`}
                              onclick={() => (customForm.arucoSource = 'http')}
                            >
                              HTTP
                            </button>
                            <button
                              type="button"
                              class={`px-2 py-1 ${customForm.arucoSource === 'nt' ? 'bg-primary-600/30 text-primary-100' : 'bg-surface-900 text-surface-300'}`}
                              onclick={() => (customForm.arucoSource = 'nt')}
                            >
                              NT
                            </button>
                          </div>
                        </div>
                      </div>
                      <div class="grid gap-2 sm:grid-cols-2">
                        <label class="flex flex-col gap-1 text-surface-200">
                          Detections array path
                          <input class={INPUT_CLASSES} bind:value={customForm.mapping.arucoListPath} placeholder="results.tags" />
                        </label>
                        <label class="flex flex-col gap-1 text-surface-200">
                          Tag id path
                          <input class={INPUT_CLASSES} bind:value={customForm.mapping.arucoId} placeholder="id" />
                        </label>
                        <label class="flex flex-col gap-1 text-surface-200">
                          Tag family path
                          <input class={INPUT_CLASSES} bind:value={customForm.mapping.arucoFamily} placeholder="family" />
                        </label>
                        <label class="flex flex-col gap-1 text-surface-200">
                          Center X path
                          <input class={INPUT_CLASSES} bind:value={customForm.mapping.arucoCenterX} placeholder="center.x" />
                        </label>
                        <label class="flex flex-col gap-1 text-surface-200">
                          Center Y path
                          <input class={INPUT_CLASSES} bind:value={customForm.mapping.arucoCenterY} placeholder="center.y" />
                        </label>
                      </div>
                      <div class="grid gap-2 sm:grid-cols-3">
                        <label class="flex flex-col gap-1 text-surface-200">
                          Pose X (m)
                          <input class={INPUT_CLASSES} bind:value={customForm.mapping.arucoTranslationX} placeholder="pose.x" />
                        </label>
                        <label class="flex flex-col gap-1 text-surface-200">
                          Pose Y (m)
                          <input class={INPUT_CLASSES} bind:value={customForm.mapping.arucoTranslationY} placeholder="pose.y" />
                        </label>
                        <label class="flex flex-col gap-1 text-surface-200">
                          Pose Z (m)
                          <input class={INPUT_CLASSES} bind:value={customForm.mapping.arucoTranslationZ} placeholder="pose.z" />
                        </label>
                      </div>
                      <div class="mt-2 grid gap-2 sm:grid-cols-3">
                        <label class="flex flex-col gap-1 text-surface-200">
                          Roll (°)
                          <input class={INPUT_CLASSES} bind:value={customForm.mapping.arucoRotationRoll} placeholder="pose.roll" />
                        </label>
                        <label class="flex flex-col gap-1 text-surface-200">
                          Pitch (°)
                          <input class={INPUT_CLASSES} bind:value={customForm.mapping.arucoRotationPitch} placeholder="pose.pitch" />
                        </label>
                        <label class="flex flex-col gap-1 text-surface-200">
                          Yaw (°)
                          <input class={INPUT_CLASSES} bind:value={customForm.mapping.arucoRotationYaw} placeholder="pose.yaw" />
                        </label>
                      </div>
                    </section>
                  {/if}
                </div>
              {/if}
            </div>

            {#if modalView === 'manual'}
              <div class="space-y-2 rounded border border-surface-800 bg-surface-950/50 p-3 lg:sticky lg:top-16 lg:h-fit">
                <div class="flex flex-wrap items-center justify-between gap-3">
                  <div>
                    <p class="font-semibold text-surface-100">Call endpoint / preview</p>
                    <p class="text-xs text-surface-500">Fetch the data endpoint and evaluate mappings. CORS must permit this device.</p>
                    {#if customTest.url}
                      <p class="text-[0.7rem] text-surface-500">Last target: {customTest.url}</p>
                    {/if}
                  </div>
                  <button class="btn btn-2xs preset-filled-primary-500" type="button" onclick={onTestEndpoint} disabled={customPending.customTest}>
                    {customPending.customTest ? 'Testing…' : 'Send test request'}
                  </button>
                </div>
                {#if customTest.error}
                  <div class="rounded border border-amber-600 bg-amber-600/20 px-3 py-2 text-xs text-amber-100">
                    {customTest.error}
                  </div>
                {/if}
                {#if customTest.preview}
                  <div class="rounded border border-surface-800 bg-surface-900/70 p-3">
                    <p class="text-xs uppercase tracking-[0.25em] text-surface-500">Preview</p>
                    {#if customTest.preview.pose}
                      <div class="mt-2 text-xs text-surface-300">
                        <p class="font-semibold text-surface-100">Pose</p>
                        {#if customTest.preview.pose.translation}
                          <p>Translation: {JSON.stringify(customTest.preview.pose.translation)}</p>
                        {/if}
                        {#if customTest.preview.pose.rotation}
                          <p>Rotation: {JSON.stringify(customTest.preview.pose.rotation)}</p>
                        {/if}
                        {#if customTest.preview.pose.timestamp}
                          <p>Timestamp: {String(customTest.preview.pose.timestamp)}</p>
                        {/if}
                        {#if customTest.preview.pose.latencyMs}
                          <p>Latency: {String(customTest.preview.pose.latencyMs)}</p>
                        {/if}
                      </div>
                    {/if}
                    {#if customTest.preview.arucoTags}
                      <div class="mt-3 text-xs text-surface-300">
                        <p class="font-semibold text-surface-100">ArUco tags ({customTest.preview.arucoTags.length} shown)</p>
                        <div class="mt-1 space-y-1">
                          {#each customTest.preview.arucoTags as tag, idx (tag.id ?? idx)}
                            <div class="rounded border border-surface-800/70 bg-surface-950/40 p-2">
                              <p class="text-[0.7rem] uppercase tracking-[0.2em] text-surface-500">Detection {idx + 1}</p>
                              <p>ID: {String(tag.id ?? '—')}</p>
                              {#if tag.family}<p>Family: {String(tag.family)}</p>{/if}
                              {#if tag.center}<p>Center: {JSON.stringify(tag.center)}</p>{/if}
                              {#if tag.translation}<p>Pose: {JSON.stringify(tag.translation)}</p>{/if}
                              {#if tag.rotation}<p>Rotation: {JSON.stringify(tag.rotation)}</p>{/if}
                            </div>
                          {/each}
                        </div>
                      </div>
                    {/if}
                    {#if !customTest.preview.pose && (!customTest.preview.arucoTags || customTest.preview.arucoTags.length === 0)}
                      <p class="text-xs text-surface-500">No fields resolved with the current mappings.</p>
                    {/if}
                  </div>
                {/if}
                {#if customTest.response}
                  <details class="rounded border border-surface-800 bg-surface-950/50 p-3 text-xs text-surface-300">
                    <summary class="cursor-pointer font-semibold text-surface-100">Raw response</summary>
                    <pre class="mt-2 overflow-auto rounded bg-black/40 p-2 text-[0.7rem]">{JSON.stringify(customTest.response, null, 2)}</pre>
                  </details>
                {/if}
              </div>
            {:else}
              <div class="space-y-3 rounded border border-surface-800 bg-surface-950/50 p-4 text-xs text-surface-300 lg:sticky lg:top-16 lg:h-fit">
                <p class="font-semibold text-surface-100">What happens next</p>
                <p class="text-surface-500">Helios stores the peer, pings the API base, and surfaces the stream link.</p>
                <div class="space-y-2">
                  <div class="rounded border border-surface-800/70 bg-surface-950/60 p-3">
                    <p class="text-micro-tight uppercase tracking-[0.3em] text-surface-500">API base</p>
                    <p class="mt-1 text-surface-200">{customForm.apiBaseUrl || 'Auto-filled'}</p>
                  </div>
                  <div class="rounded border border-surface-800/70 bg-surface-950/60 p-3">
                    <p class="text-micro-tight uppercase tracking-[0.3em] text-surface-500">Stream</p>
                    <p class="mt-1 text-surface-200">{customForm.streamUrl || 'Auto-filled'}</p>
                  </div>
                </div>
                <p class="text-[0.7rem] text-surface-500">Need custom mappings? Switch to the Manual tab.</p>
              </div>
            {/if}
          </div>

          <div class="flex flex-wrap items-center justify-between gap-3">
            <p class="text-xs text-surface-500">
              {modalView === 'manual'
                ? 'Start with base URL plus one data endpoint or NT path; add mapping sections only when needed.'
                : 'Device IP is enough. Advanced fields are optional and can be edited later.'}
            </p>
            <div class="flex gap-2">
              <button class="btn btn-2xs preset-tonal" type="button" onclick={() => onSelectView('discover')}>
                Back to discovery
              </button>
              <button class="btn btn-2xs preset-tonal" type="button" onclick={onTestConnection} disabled={customPending.probe}>
                {customPending.probe ? 'Testing…' : 'Test connection'}
              </button>
              <button class="btn btn-sm preset-filled-primary-500" type="submit" disabled={customPending.customSave}>
                {customPending.customSave ? 'Saving…' : customForm.peerId ? 'Update peer' : 'Save peer'}
              </button>
            </div>
          </div>
        </form>
      {/if}
    </div>
  </div>
</div>
