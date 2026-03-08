<script lang="ts">
  import { onDestroy, onMount } from 'svelte';
  import { Tabs } from '@skeletonlabs/skeleton-svelte';
  import type { IconDefinition } from '@fortawesome/free-solid-svg-icons';
  import { faBolt, faCamera, faCloudArrowDown, faImages, faNetworkWired, faPuzzlePiece, faMicrochip } from '@fortawesome/free-solid-svg-icons';
  import { connectRealtimeUpdatesStream, type RealtimeUpdateEvent } from '$lib/api/realtimeUpdates';
  import ApiEndpointPanel from './components/ApiEndpointPanel.svelte';
  import CameraLayoutPanel from './components/CameraLayoutPanel.svelte';
  import NetworkingWorkspacePanel from './components/NetworkingWorkspacePanel.svelte';
  import SnapshotsPanel from './components/SnapshotsPanel.svelte';
  import RestartPanel from './components/RestartPanel.svelte';
  import UpdaterPanel from './components/UpdaterPanel.svelte';
  import PluginsPanel from './components/PluginsPanel.svelte';
  import UsbPowerPanel from './components/UsbPowerPanel.svelte';
  import BootloaderPanel from './components/BootloaderPanel.svelte';
  import { apiFetch, extractError } from './api';
  import { deviceSettingsStore, type DeviceSettingsState } from './deviceSettingsStore';
  import { rigLayoutStore, type RigLayoutState } from '$lib/stores/rigLayout';
  import FaIcon from '$lib/components/icons/FaIcon.svelte';
  import { Panel } from '$lib';
  import type { BootloaderStatus } from './types';

  type WorkspaceTabId = 'network' | 'rig' | 'snapshots' | 'updater' | 'plugins' | 'usb-power' | 'firmware';
  type WorkspaceGroup = {
    id: 'connectivity' | 'operations';
    label: string;
    detail: string;
    tabs: WorkspaceTabId[];
  };

  const workspaceTabs: Array<{ id: WorkspaceTabId; label: string; detail: string }> = [
    { id: 'network', label: 'Networking', detail: 'Identity + interfaces' },
    { id: 'rig', label: 'Rig layout', detail: 'Viewer + chassis' },
    { id: 'snapshots', label: 'Snapshots', detail: 'State archives' },
    { id: 'updater', label: 'Updater', detail: 'Stage + apply releases' },
    { id: 'plugins', label: 'Plugins', detail: 'Install + manage' },
    { id: 'usb-power', label: 'USB power', detail: 'External port rails' },
    { id: 'firmware', label: 'Firmware', detail: 'Bootloader updates' }
  ];

  const workspaceGroups: WorkspaceGroup[] = [
    { id: 'connectivity', label: 'Connectivity', detail: 'Radio and rig layout', tabs: ['network', 'rig'] },
    { id: 'operations', label: 'Operations', detail: 'State + runtime controls', tabs: ['snapshots', 'updater', 'plugins', 'usb-power', 'firmware'] }
  ];

  const workspaceTabIcons: Record<WorkspaceTabId, IconDefinition> = {
    network: faNetworkWired,
    rig: faCamera,
    snapshots: faImages,
    updater: faCloudArrowDown,
    plugins: faPuzzlePiece,
    'usb-power': faBolt,
    firmware: faMicrochip
  };

  let activeWorkspaceTab = $state<WorkspaceTabId>('network');
  let bootloaderStatus = $state<BootloaderStatus | null>(null);
  let bootloaderError = $state<string | null>(null);
  let bootloaderLoading = $state(false);
  let liveUpdatesCleanup: (() => void) | null = null;
  let liveUpdatesReconnectHandle: number | null = null;
  let liveUpdatesRefreshHandle: number | null = null;
  let liveUpdatesNonce = 0;

  const LIVE_UPDATES_RECONNECT_MS = 1_500;
  const LIVE_UPDATES_REFRESH_DEBOUNCE_MS = 400;

  const deviceState = $derived($deviceSettingsStore as DeviceSettingsState);
  const rigState = $derived($rigLayoutStore as RigLayoutState);

  const workspaceSurfaces = $derived.by(() =>
    workspaceTabs.map((tab) => ({
      ...tab,
      component: getWorkspaceComponent(tab.id)
    }))
  );

  onMount(() => {
    void deviceSettingsStore.load().catch(() => {
      // handled by child panels
    });
    void rigLayoutStore.refresh({ force: true }).catch(() => {
      // viewer panels show fallbacks
    });
    void refreshBootloaderStatus();
    connectLiveUpdates();
  });

  onDestroy(() => {
    disconnectLiveUpdates();
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
    if (event.kind === 'api') {
      return false;
    }
    return event.kind === 'device' || event.kind === 'settings' || event.kind === 'imu' || event.kind === 'media';
  }

  function refreshSettingsFromLiveUpdate(): void {
    void deviceSettingsStore.load({ quiet: true, force: true }).catch(() => {
      // panels surface errors from the store
    });
    void rigLayoutStore.refresh({ force: true }).catch(() => {
      // best-effort
    });
    void refreshBootloaderStatus();
  }

  function scheduleLiveUpdatesRefresh(): void {
    if (liveUpdatesRefreshHandle != null) return;
    liveUpdatesRefreshHandle = window.setTimeout(() => {
      liveUpdatesRefreshHandle = null;
      if (document.hidden) return;
      refreshSettingsFromLiveUpdate();
    }, LIVE_UPDATES_REFRESH_DEBOUNCE_MS);
  }

  function scheduleLiveUpdatesReconnect(): void {
    if (liveUpdatesReconnectHandle != null) return;
    liveUpdatesReconnectHandle = window.setTimeout(() => {
      liveUpdatesReconnectHandle = null;
      connectLiveUpdates();
    }, LIVE_UPDATES_RECONNECT_MS);
  }

  function disconnectLiveUpdates(): void {
    liveUpdatesNonce += 1;
    if (liveUpdatesReconnectHandle != null) {
      clearTimeout(liveUpdatesReconnectHandle);
      liveUpdatesReconnectHandle = null;
    }
    if (liveUpdatesRefreshHandle != null) {
      clearTimeout(liveUpdatesRefreshHandle);
      liveUpdatesRefreshHandle = null;
    }
    liveUpdatesCleanup?.();
    liveUpdatesCleanup = null;
  }

  function connectLiveUpdates(): void {
    const nonce = (liveUpdatesNonce += 1);
    if (liveUpdatesReconnectHandle != null) {
      clearTimeout(liveUpdatesReconnectHandle);
      liveUpdatesReconnectHandle = null;
    }
    liveUpdatesCleanup?.();
    liveUpdatesCleanup = null;
    liveUpdatesCleanup = connectRealtimeUpdatesStream({
      onChange: (event) => {
        if (nonce !== liveUpdatesNonce) return;
        if (!shouldApplyLiveUpdate(event)) return;
        window.dispatchEvent(new CustomEvent('helios:settings-realtime-update', { detail: event }));
        scheduleLiveUpdatesRefresh();
      },
      onClose: () => {
        if (nonce !== liveUpdatesNonce) return;
        scheduleLiveUpdatesReconnect();
      },
      onError: () => {
        if (nonce !== liveUpdatesNonce) return;
        scheduleLiveUpdatesReconnect();
      }
    });
  }

  function getWorkspaceComponent(tab: WorkspaceTabId) {
    switch (tab) {
      case 'network':
        return NetworkingWorkspacePanel;
      case 'rig':
        return CameraLayoutPanel;
      case 'snapshots':
        return SnapshotsPanel;
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
      bootloaderStatus = await apiFetch<BootloaderStatus>('/device/bootloader');
    } catch (err) {
      bootloaderError = extractError(err);
    } finally {
      bootloaderLoading = false;
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
