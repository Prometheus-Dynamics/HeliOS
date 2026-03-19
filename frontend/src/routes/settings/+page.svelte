<script lang="ts">
  import { page } from '$app/stores';
  import { onMount } from 'svelte';
  import { Tabs } from '@skeletonlabs/skeleton-svelte';
  import type { IconDefinition } from '@fortawesome/free-solid-svg-icons';
  import { faBolt, faCamera, faCloudArrowDown, faStethoscope, faNetworkWired, faPuzzlePiece, faMicrochip, faTriangleExclamation } from '@fortawesome/free-solid-svg-icons';
  import { subscribeDomainInvalidations } from '$lib/api/invalidation';
  import { realtimeUpdateMatchesKind, type RealtimeUpdateEvent } from '$lib/api/realtimeUpdates';
  import ApiEndpointPanel from './components/ApiEndpointPanel.svelte';
  import CameraLayoutPanel from './components/CameraLayoutPanel.svelte';
  import NetworkingWorkspacePanel from './components/NetworkingWorkspacePanel.svelte';
  import DiagnosticsPanel from './components/DiagnosticsPanel.svelte';
  import RestartPanel from './components/RestartPanel.svelte';
  import UpdaterPanel from './components/UpdaterPanel.svelte';
  import PluginsPanel from './components/PluginsPanel.svelte';
  import UsbPowerPanel from './components/UsbPowerPanel.svelte';
  import BootloaderPanel from './components/BootloaderPanel.svelte';
  import { extractError } from './api';
  import { bootloaderStatusResource, osHealthStatusResource, type OsHealthStatus } from '$lib/api/deviceStatusResources';
  import { deviceSettingsStore, type DeviceSettingsState } from './deviceSettingsStore';
  import { rigLayoutStore, type RigLayoutState } from '$lib/stores/rigLayout';
  import FaIcon from '$lib/components/icons/FaIcon.svelte';
  import Panel from '$lib/components/Panel.svelte';
  import type { BootloaderStatus } from './types';

  type WorkspaceTabId = 'network' | 'rig' | 'diagnostics' | 'updater' | 'plugins' | 'usb-power' | 'firmware';
  type WorkspaceGroup = {
    id: 'connectivity' | 'operations';
    label: string;
    detail: string;
    tabs: WorkspaceTabId[];
  };

  const workspaceTabs: Array<{ id: WorkspaceTabId; label: string; detail: string }> = [
    { id: 'network', label: 'Networking', detail: 'Identity + interfaces' },
    { id: 'rig', label: 'Rig layout', detail: 'Viewer + chassis' },
    { id: 'diagnostics', label: 'Diagnostics', detail: 'Health + archives' },
    { id: 'updater', label: 'Updater', detail: 'Stage + apply releases' },
    { id: 'plugins', label: 'Plugins', detail: 'Install + manage' },
    { id: 'usb-power', label: 'USB power', detail: 'External port rails' },
    { id: 'firmware', label: 'Firmware', detail: 'Bootloader updates' }
  ];

  const workspaceGroups: WorkspaceGroup[] = [
    { id: 'connectivity', label: 'Connectivity', detail: 'Radio and rig layout', tabs: ['network', 'rig'] },
    { id: 'operations', label: 'Operations', detail: 'State + runtime controls', tabs: ['diagnostics', 'updater', 'plugins', 'usb-power', 'firmware'] }
  ];

  const workspaceTabIcons: Record<WorkspaceTabId, IconDefinition> = {
    network: faNetworkWired,
    rig: faCamera,
    diagnostics: faStethoscope,
    updater: faCloudArrowDown,
    plugins: faPuzzlePiece,
    'usb-power': faBolt,
    firmware: faMicrochip
  };

  const LEGACY_WORKSPACE_TAB_ALIASES: Record<string, WorkspaceTabId> = {
    snapshots: 'diagnostics'
  };

  let activeWorkspaceTab = $state<WorkspaceTabId>('network');
  let bootloaderStatus = $state<BootloaderStatus | null>(null);
  let bootloaderError = $state<string | null>(null);
  let bootloaderLoading = $state(false);
  let osHealthStatus = $state<OsHealthStatus | null>(null);
  let lastAppliedQueryTab = $state<WorkspaceTabId | null>(null);
  const LIVE_UPDATES_REFRESH_DEBOUNCE_MS = 400;

  const deviceState = $derived($deviceSettingsStore as DeviceSettingsState);
  const rigState = $derived($rigLayoutStore as RigLayoutState);

  const workspaceSurfaces = $derived.by(() =>
    workspaceTabs.map((tab) => ({
      ...tab,
      component: getWorkspaceComponent(tab.id)
    }))
  );
  const osRelease = $derived(deviceState.data?.os_release ?? null);
  const osVersionLabel = $derived.by(() => {
    const release = osRelease;
    if (!release) return 'Unknown';
    return release.version_id?.trim() || release.pretty_name?.trim() || 'Unknown';
  });
  const osBuildLabel = $derived.by(() => {
    const buildId = osRelease?.build_id?.trim();
    if (!buildId || buildId.length === 0) return null;
    return buildId === osVersionLabel ? null : buildId;
  });
  const activeRootLabel = $derived.by(() => {
    const activeRoot = osRelease?.active_root?.trim();
    return activeRoot && activeRoot.length > 0 ? activeRoot : 'Unknown';
  });

  onMount(() => {
    const cachedBootloader = bootloaderStatusResource.read();
    if (cachedBootloader?.data) {
      bootloaderStatus = cachedBootloader.data;
    }
    const cachedOsHealth = osHealthStatusResource.read();
    if (cachedOsHealth?.data) {
      osHealthStatus = cachedOsHealth.data;
    }
    void deviceSettingsStore.load().catch(() => {
      // handled by child panels
    });
    void rigLayoutStore.refresh({ force: true }).catch(() => {
      // viewer panels show fallbacks
    });
    void refreshBootloaderStatus();
    void refreshOsHealthStatus();
    return subscribeDomainInvalidations(
      ['device', 'settings', 'imu', 'media'],
      (event) => {
        if (document.hidden) return;
        if (!shouldApplyLiveUpdate(event)) return;
        refreshSettingsFromLiveUpdate();
      },
      { debounceMs: LIVE_UPDATES_REFRESH_DEBOUNCE_MS }
    );
  });

  $effect(() => {
    const requested = normalizeWorkspaceTab($page.url.searchParams.get('tab'));
    if (requested && requested !== lastAppliedQueryTab) {
      activeWorkspaceTab = requested;
      lastAppliedQueryTab = requested;
    }
  });

  function shouldApplyLiveUpdate(event: RealtimeUpdateEvent): boolean {
    if (
      event.path.startsWith('/v1/device') ||
      event.path.startsWith('/v1/peripherals') ||
      event.path.startsWith('/v1/plugins') ||
      event.path.startsWith('/v1/ota') ||
      event.path.startsWith('/v1/media')
    ) {
      return true;
    }
    if (realtimeUpdateMatchesKind(event, 'api')) {
      return false;
    }
    return (
      realtimeUpdateMatchesKind(event, 'device') ||
      realtimeUpdateMatchesKind(event, 'settings') ||
      realtimeUpdateMatchesKind(event, 'imu') ||
      realtimeUpdateMatchesKind(event, 'media')
    );
  }

  function refreshSettingsFromLiveUpdate(): void {
    void deviceSettingsStore.load({ quiet: true, force: true }).catch(() => {
      // panels surface errors from the store
    });
    void rigLayoutStore.refresh({ force: true }).catch(() => {
      // best-effort
    });
    void refreshBootloaderStatus();
    void refreshOsHealthStatus();
  }

  function normalizeWorkspaceTab(value: string | null): WorkspaceTabId | null {
    if (!value) return null;
    const normalized = value.trim().toLowerCase();
    if (normalized in LEGACY_WORKSPACE_TAB_ALIASES) {
      return LEGACY_WORKSPACE_TAB_ALIASES[normalized];
    }
    const match = workspaceTabs.find((tab) => tab.id === normalized);
    return match?.id ?? null;
  }

  function getWorkspaceComponent(tab: WorkspaceTabId) {
    switch (tab) {
      case 'network':
        return NetworkingWorkspacePanel;
      case 'rig':
        return CameraLayoutPanel;
      case 'diagnostics':
        return DiagnosticsPanel;
      case 'plugins':
        return PluginsPanel;
      case 'usb-power':
        return UsbPowerPanel;
      case 'firmware':
        return BootloaderPanel;
      default:
        return UpdaterPanel;
    }
  }

  function handleWorkspaceChange({ value }: { value: string | null }): void {
    if (!value) return;
    const match = workspaceTabs.find((tab) => tab.id === value);
    if (!match) return;
    activeWorkspaceTab = match.id;
  }

  async function refreshBootloaderStatus(): Promise<void> {
    bootloaderLoading = true;
    bootloaderError = null;
    try {
      bootloaderStatus = await bootloaderStatusResource.refresh();
    } catch (err) {
      bootloaderError = extractError(err);
    } finally {
      bootloaderLoading = false;
    }
  }

  async function refreshOsHealthStatus(): Promise<void> {
    try {
      osHealthStatus = await osHealthStatusResource.refresh();
    } catch {
      osHealthStatus = null;
    }
  }
</script>

<section class="flex h-full min-h-0 flex-1 flex-col gap-6 overflow-hidden">
  {#if deviceState.error || rigState.error}
    <div class="border border-error-500/50 bg-error-500/10 px-4 py-3 text-sm text-error-100">
      {#if deviceState.error}
        <p>Device settings · {deviceState.error}</p>
      {/if}
      {#if rigState.error}
        <p>Rig layout · {rigState.error}</p>
      {/if}
    </div>
  {/if}
  {#if bootloaderStatus?.needs_update}
    <div class="rounded border border-warning-500/40 bg-warning-500/10 px-4 py-3 text-sm text-warning-100">
      <div class="flex flex-wrap items-center justify-between gap-2">
        <p>Bootloader firmware update required. Peripheral LEDs may stay offline until updated.</p>
        <button class="btn btn-xs variant-soft" onclick={() => (activeWorkspaceTab = 'firmware')}>
          Open firmware updater
        </button>
      </div>
    </div>
  {/if}
  {#if bootloaderLoading}
    <div class="rounded border border-surface-700/60 bg-surface-900/40 px-4 py-3 text-sm text-surface-300">
      Checking bootloader status…
    </div>
  {/if}
  {#if bootloaderError}
    <div class="rounded border border-warning-500/40 bg-warning-500/10 px-4 py-3 text-sm text-warning-100">
      Bootloader status unavailable · {bootloaderError}
    </div>
  {/if}

  <Tabs
    value={activeWorkspaceTab}
    onValueChange={handleWorkspaceChange}
    activationMode="manual"
    base="flex w-full flex-1 min-h-0 h-full flex-col gap-4 overflow-hidden lg:min-h-0 lg:flex-row"
    listBase="order-2 w-full shrink-0 lg:order-1 lg:max-w-sm"
    listBorder=""
    listMargin=""
    listGap=""
    contentBase="order-1 flex-1 min-h-0 h-full overflow-hidden lg:order-2"
  >
    {#snippet list()}
      <aside class="space-y-4 border border-surface-700/60 bg-surface-900/30 p-4 text-sm text-surface-300 min-h-0 overflow-y-auto lg:h-full">
        <div class="border border-surface-700/60 bg-surface-950/45 p-3">
          <div class="flex items-start justify-between gap-3">
            <div class="min-w-0">
              <p class="text-xs font-semibold uppercase tracking-[0.24em] text-surface-500">OS version</p>
              <p class="mt-1 text-sm font-semibold text-surface-50">{osVersionLabel}</p>
              {#if osBuildLabel}
                <p class="mt-1 break-all font-mono text-[0.7rem] text-surface-400">{osBuildLabel}</p>
              {/if}
            </div>
            <div class="min-w-0 text-right">
              <p class="text-xs font-semibold uppercase tracking-[0.24em] text-surface-500">Active root</p>
              <p class="mt-1 font-mono text-sm font-semibold text-surface-50">{activeRootLabel}</p>
            </div>
          </div>
        </div>
        {#each workspaceGroups as group (group.id)}
          <div class="space-y-2 border border-surface-700/60 bg-surface-900/40 p-3">
            <div class="flex items-center justify-between gap-2">
              <p class="text-xs uppercase tracking-[0.25em] text-surface-500">{group.label}</p>
              <span class="text-[0.7rem] text-surface-500">{group.detail}</span>
            </div>
            <div class="space-y-1">
              {#each workspaceSurfaces.filter((tab) => group.tabs.includes(tab.id)) as tab (tab.id)}
                <Tabs.Control
                  value={tab.id}
                  base="w-full text-left"
                  padding="p-0"
                  translateX=""
                  classes="w-full"
                  labelBase="w-full"
                  stateLabelInactive=""
                  stateLabelActive=""
                >
                  <div
                    class={`flex items-start justify-between gap-3 border px-3 py-2 transition ${
                      activeWorkspaceTab === tab.id
                        ? 'border-primary-300/70 bg-primary-500/10 text-primary-50'
                        : 'border-surface-700 bg-surface-950/40 text-surface-100 hover:border-surface-500'
                    }`}
                  >
                    <div class="flex items-center gap-2">
                      <div class={`grid h-8 w-8 place-items-center border ${activeWorkspaceTab === tab.id ? 'border-primary-300/60 bg-primary-500/15 text-primary-50' : 'border-surface-700/80 bg-surface-900/60 text-surface-400'}`}>
                        <FaIcon icon={workspaceTabIcons[tab.id]} class="h-4 w-4" />
                      </div>
                      <div>
                        <p class="text-sm font-semibold">{tab.label}</p>
                        <p class="text-[0.7rem] text-surface-500">{tab.detail}</p>
                      </div>
                    </div>
                    {#if tab.id === 'diagnostics' && osHealthStatus?.issues?.length}
                      <span class="rounded-full border border-error-400/50 bg-error-500/10 px-2 py-0.5 text-[0.65rem] font-semibold uppercase tracking-[0.2em] text-error-200">
                        {osHealthStatus.issues.length}
                      </span>
                    {/if}
                  </div>
                </Tabs.Control>
              {/each}
            </div>
          </div>
        {/each}

        <div class="border-t border-surface-800/60 pt-3">
          <ApiEndpointPanel />
          <div class="mt-3">
            <RestartPanel />
          </div>
        </div>
      </aside>
    {/snippet}

    {#snippet content()}
      <Panel
        className="flex min-h-0 h-full flex-1 flex-col overflow-hidden"
        tone="default"
      >
        <div class="flex-1 min-h-0 h-full overflow-y-auto">
          {#each workspaceSurfaces as tab (tab.id)}
            <Tabs.Panel value={tab.id} classes="flex h-full min-h-0 flex-col">
              {@const TabComponent = tab.component}
              <div class="flex h-full min-h-0 w-full flex-col gap-4 px-1 sm:px-0 lg:gap-5">
                <TabComponent />
              </div>
            </Tabs.Panel>
          {/each}
        </div>
      </Panel>
    {/snippet}
  </Tabs>
</section>
