<script lang="ts">
  import { Panel } from '$lib';
  import type { PeerFilterDefinition, PeerFilterOption } from '$lib/features/peers/types';
  import type { PeerProbeResponse, PeerSummary } from '$lib/types/peer';
  import {
    displayName,
    formatCapabilities,
    formatCpuTelemetry,
    formatEndpoints,
    formatGpuTelemetry,
    formatLatency,
    formatMemoryTelemetry,
    formatPoseRotation,
    formatPoseTranslation,
    formatProbeMetric,
    formatStatus,
    formatTelemetryAge,
    formatTimestamp,
    integrationLabel,
    integrationLogo,
    managementTarget,
    peerStreamUrls,
    statusBadge,
    telemetryAvailable
  } from '$lib/features/peers/utils';

  type Props = {
    isInitialLoading: boolean;
    peers: PeerSummary[];
    filteredPeers: PeerSummary[];
    viewFilter: PeerFilterOption;
    filters: PeerFilterDefinition[];
    searchQuery: string;
    peerProbeCache: Record<string, PeerProbeResponse | null>;
    customPending: { probe: boolean };
    onOpenMapping: (peer: PeerSummary) => void;
    onSyncPipelines: (peer: PeerSummary) => void;
    onProbePeer: (peer: PeerSummary) => void;
    onOpenManagement: (peer: PeerSummary) => void;
    onRemove: (peerId: string) => void;
    isRemoving: (peerId: string) => boolean;
    isSyncingPipelines: (peerId: string) => boolean;
  };

  const {
    isInitialLoading,
    peers,
    filteredPeers,
    viewFilter,
    filters,
    searchQuery,
    peerProbeCache,
    customPending,
    onOpenMapping,
    onSyncPipelines,
    onProbePeer,
    onOpenManagement,
    onRemove,
    isRemoving,
    isSyncingPipelines
  }: Props = $props();

  const resolvedFilterLabel = (value: string) => filters.find((filter) => filter.id === value)?.label ?? 'selected';
</script>

<Panel title="Known peers" className="flex flex-1 flex-col min-h-0">
  {#if isInitialLoading}
    <div class="grid gap-3 lg:grid-cols-2">
      {#each Array.from({ length: 4 }) as _, idx (idx)}
        <article class="rounded border border-surface-800 bg-surface-900/60 p-4 text-sm text-surface-400 animate-pulse">
          <div class="flex items-center gap-3">
            <div class="h-10 w-10 rounded bg-surface-800/70"></div>
            <div class="flex-1 space-y-2">
              <div class="h-4 w-1/2 rounded bg-surface-800/70"></div>
              <div class="h-3 w-1/3 rounded bg-surface-800/60"></div>
            </div>
          </div>
          <div class="mt-4 grid gap-3 lg:grid-cols-2">
            <div class="h-4 w-full rounded bg-surface-800/60"></div>
            <div class="h-4 w-full rounded bg-surface-800/60"></div>
            <div class="h-4 w-2/3 rounded bg-surface-800/60"></div>
            <div class="h-4 w-1/2 rounded bg-surface-800/60"></div>
          </div>
        </article>
      {/each}
    </div>
  {:else if peers.length === 0}
    <div class="rounded border border-dashed border-surface-700 bg-surface-900/40 px-4 py-6 text-center text-sm text-surface-300">
      No peers registered yet. Use discovery or registration to add devices to the cluster.
    </div>
  {:else if filteredPeers.length === 0}
    <div class="rounded border border-surface-800 bg-surface-950/30 px-4 py-6 text-center text-sm text-surface-400">
      {#if searchQuery.trim()}
        No peers match “{searchQuery.trim()}”.
      {:else}
        No peers match the “{resolvedFilterLabel(viewFilter)}” filter.
      {/if}
    </div>
  {:else}
    <div class="space-y-4">
      {#each filteredPeers as peer (peer.id)}
        <article class="rounded border border-surface-800 bg-surface-900/50 p-4 text-sm text-surface-300 shadow shadow-black/30">
          <div class="flex flex-col gap-4 md:flex-row md:items-start md:justify-between">
            <div class="flex items-start gap-3">
              <img src={integrationLogo(peer.integration.kind)} alt="" class="h-10 w-10 shrink-0 rounded border border-surface-800/70 bg-surface-950 object-contain p-1" />
              <div>
                <div class="flex flex-wrap items-center gap-2">
                  <span class="text-lg font-semibold text-surface-50">{displayName(peer)}</span>
                  <span class="text-xs uppercase tracking-[0.3em] text-surface-500">{integrationLabel(peer.integration.kind)}</span>
                </div>
                <p class="text-xs text-surface-500">{peer.id}</p>
                {#if peer.version}
                  <p class="text-xs text-surface-500">v{peer.version}</p>
                {/if}
              </div>
            </div>
            <div class="flex flex-col items-start gap-1 text-xs text-surface-500 md:items-end md:text-right">
              <span class={`inline-flex items-center rounded px-2 py-1 text-xs font-semibold ${statusBadge(peer.status)}`}>
                {formatStatus(peer.status)}
              </span>
              <span>Last seen {formatTimestamp(peer.lastSeenAt)}</span>
            </div>
          </div>

          <div class="mt-4 grid gap-4 lg:grid-cols-4">
            <section class="rounded border border-surface-800/50 bg-surface-950/20 p-3">
              <p class="text-micro uppercase tracking-[0.3em] text-surface-500">Connection</p>
              <dl class="mt-2 space-y-2 text-surface-200">
                <div>
                  <dt class="text-xs text-surface-500">API base</dt>
                  <dd>
                    <a class="text-primary-300 hover:text-primary-100" href={peer.apiBaseUrl} target="_blank" rel="noreferrer">
                      {peer.apiBaseUrl}
                    </a>
                  </dd>
                </div>
                <div>
                  <dt class="text-xs text-surface-500">Endpoints</dt>
                  <dd>{formatEndpoints(peer.endpoints)}</dd>
                </div>
                <div>
                  <dt class="text-xs text-surface-500">Latency</dt>
                  <dd>{formatLatency(peer.latencyMs)}</dd>
                </div>
                {#if peerProbeCache[peer.id]}
                  <div>
                    <dt class="text-xs text-surface-500">Last probe</dt>
                    <dd class="space-y-0.5">
                      <p>API: {formatProbeMetric(peerProbeCache[peer.id]?.api)}</p>
                      <p>UI: {formatProbeMetric(peerProbeCache[peer.id]?.management)}</p>
                      <p>
                        Streams: {(peerProbeCache[peer.id]?.streams ?? []).filter((item) => item.ok).length}/{(peerProbeCache[peer.id]?.streams ?? []).length}
                      </p>
                      {#if peerProbeCache[peer.id]?.nt4}
                        <p>
                          NT4: {peerProbeCache[peer.id]?.nt4?.ok ? 'ok' : 'fail'} · {peerProbeCache[peer.id]?.nt4?.host}:{peerProbeCache[peer.id]?.nt4?.port}
                        </p>
                        {#if (peerProbeCache[peer.id]?.nt4?.roots ?? []).length > 0}
                          <p class="text-[0.7rem] text-surface-500">
                            Roots: {(peerProbeCache[peer.id]?.nt4?.roots ?? []).join(', ')}
                          </p>
                        {/if}
                      {/if}
                    </dd>
                  </div>
                {/if}
                <div>
                  <dt class="text-xs text-surface-500">Capabilities</dt>
                  <dd>{formatCapabilities(peer.capabilities)}</dd>
                </div>
              </dl>
            </section>

            <section class="rounded border border-surface-800/50 bg-surface-950/20 p-3">
              <p class="text-micro uppercase tracking-[0.3em] text-surface-500">Integration</p>
              <dl class="mt-2 space-y-2 text-surface-200">
                <div>
                  <dt class="text-xs text-surface-500">Management UI</dt>
                  <dd>
                    {#if managementTarget(peer)}
                      <a class="text-primary-300 hover:text-primary-100" href={managementTarget(peer)} target="_blank" rel="noreferrer">
                        {managementTarget(peer)}
                      </a>
                    {:else}
                      <span class="text-surface-500">Not provided</span>
                    {/if}
                  </dd>
                </div>
                <div>
                  <dt class="text-xs text-surface-500">Camera stream</dt>
                  <dd>
                    {#if peerStreamUrls(peer).length > 0}
                      <div class="space-y-1">
                        {#each peerStreamUrls(peer).slice(0, 6) as url (url)}
                          <div class="flex items-center justify-between gap-2">
                            <a class="min-w-0 truncate text-primary-300 hover:text-primary-100" href={url} target="_blank" rel="noreferrer">
                              {url}
                            </a>
                          </div>
                        {/each}
                        {#if peerStreamUrls(peer).length > 6}
                          <p class="text-[0.7rem] text-surface-500">Showing 6/{peerStreamUrls(peer).length} stream(s).</p>
                        {/if}
                        <p class="text-[0.7rem] text-surface-500">
                          Tip: Set the peer camera output to a high resolution and bitrate (MJPEG) for best pipeline results.
                        </p>
                      </div>
                    {:else}
                      <span class="text-surface-500">Not provided</span>
                    {/if}
                  </dd>
                </div>
                <div>
                  <dt class="text-xs text-surface-500">Custom mapping</dt>
                  <dd class="space-y-1">
                    {#if peer.integration.custom}
                      {#if peer.integration.custom.apiEndpoint}
                        <p class="text-xs text-surface-300">API: {peer.integration.custom.apiEndpoint}</p>
                      {/if}
                      {#if peer.integration.custom.networkTable}
                        <p class="text-xs text-surface-300">NT: {peer.integration.custom.networkTable}</p>
                      {/if}
                      {#if peer.integration.custom.telemetryEndpoint}
                        <p class="text-xs text-surface-300">Telemetry: {peer.integration.custom.telemetryEndpoint}</p>
                      {/if}
                      {#if !peer.integration.custom.apiEndpoint && !peer.integration.custom.networkTable}
                        <span class="text-surface-500">Mapping saved</span>
                      {/if}
                    {:else}
                      <span class="text-surface-500">Not configured</span>
                    {/if}
                  </dd>
                </div>
                <div>
                  <dt class="text-xs text-surface-500">Localization outputs</dt>
                  <dd class="flex flex-wrap gap-1">
                    {#if peer.integration.localizationOutputs.length === 0}
                      <span class="text-surface-500">Not provided</span>
                    {:else}
                      {#each peer.integration.localizationOutputs as output (output)}
                        <span class="rounded border border-surface-700/60 px-2 py-0.5 text-xs text-surface-200">{output}</span>
                      {/each}
                    {/if}
                  </dd>
                </div>
              </dl>
            </section>

            <section class="rounded border border-surface-800/50 bg-surface-950/20 p-3">
              <p class="text-micro uppercase tracking-[0.3em] text-surface-500">Calibration</p>
              <div class="mt-2 space-y-2 text-surface-200">
                <div>
                  <p class="text-xs text-surface-500">Position (m)</p>
                  <p>{formatPoseTranslation(peer.integration.cameraPose)}</p>
                </div>
                <div>
                  <p class="text-xs text-surface-500">Orientation (°)</p>
                  <p>{formatPoseRotation(peer.integration.cameraPose)}</p>
                </div>
              </div>
            </section>

            <section class="rounded border border-surface-800/50 bg-surface-950/20 p-3">
              <p class="text-micro uppercase tracking-[0.3em] text-surface-500">Telemetry</p>
              {#if telemetryAvailable(peer.telemetry)}
                <dl class="mt-2 grid gap-2 text-surface-200 sm:grid-cols-2">
                  <div>
                    <dt class="text-xs text-surface-500">CPU</dt>
                    <dd>{formatCpuTelemetry(peer.telemetry)}</dd>
                  </div>
                  <div>
                    <dt class="text-xs text-surface-500">Memory</dt>
                    <dd>{formatMemoryTelemetry(peer.telemetry)}</dd>
                  </div>
                  <div>
                    <dt class="text-xs text-surface-500">GPU</dt>
                    <dd>{formatGpuTelemetry(peer.telemetry)}</dd>
                  </div>
                  <div>
                    <dt class="text-xs text-surface-500">Updated</dt>
                    <dd>{formatTelemetryAge(peer.telemetry)}</dd>
                  </div>
                </dl>
              {:else}
                <p class="mt-2 text-xs text-surface-500">Telemetry unavailable for this peer.</p>
              {/if}
            </section>
          </div>

          <div class="mt-4 flex flex-wrap items-center justify-end gap-2">
            <button
              class="rounded border border-surface-600 px-3 py-1 text-xs font-semibold text-surface-100 transition hover:bg-surface-700/40"
              type="button"
              onclick={() => onOpenMapping(peer)}
            >
              Configure mapping
            </button>
            {#if peer.integration.kind === 'helios'}
              <button
                class="rounded border border-primary-500/60 px-3 py-1 text-xs font-semibold text-primary-100 transition hover:bg-primary-600/20 disabled:opacity-50"
                type="button"
                onclick={() => onSyncPipelines(peer)}
                disabled={isSyncingPipelines(peer.id)}
              >
                {isSyncingPipelines(peer.id) ? 'Syncing…' : 'Sync pipelines'}
              </button>
            {/if}
            <button
              class="rounded border border-surface-600 px-3 py-1 text-xs font-semibold text-surface-100 transition hover:bg-surface-700/40 disabled:opacity-50"
              type="button"
              onclick={() => onProbePeer(peer)}
              disabled={customPending.probe}
            >
              {customPending.probe ? 'Testing…' : 'Test connection'}
            </button>
            {#if peer.integration.kind !== 'helios'}
              <button
                class="rounded border border-primary-400/60 px-3 py-1 text-xs font-semibold text-primary-100 transition hover:bg-primary-600/20"
                type="button"
                onclick={() => onOpenManagement(peer)}
              >
                Open {integrationLabel(peer.integration.kind)} UI
              </button>
            {/if}
            <button
              class="rounded border border-red-500/60 px-3 py-1 text-xs font-semibold text-red-200 transition hover:bg-red-500/20 disabled:opacity-50"
              onclick={() => onRemove(peer.id)}
              disabled={isRemoving(peer.id)}
            >
              {isRemoving(peer.id) ? 'Removing…' : 'Remove'}
            </button>
          </div>
        </article>
      {/each}
    </div>
  {/if}
</Panel>
