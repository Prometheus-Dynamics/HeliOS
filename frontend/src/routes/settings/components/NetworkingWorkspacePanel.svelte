<script lang="ts">
  import { deviceSettingsStore, type DeviceSettingsState } from '../deviceSettingsStore';
  import type { DeviceNetworkInterfaceResponse, InterfaceForm, UpdateDeviceSettingsRequest } from '../types';
  import { REQUESTED_BY } from '../api';
  import { buildErrorMessage } from '$lib/ui/errorPolicy';
  import Nt4ExplorerModal from './Nt4ExplorerModal.svelte';

  const NT4_EXPLORER_VISIBLE = false;

  const deviceState = $derived($deviceSettingsStore as DeviceSettingsState);

  let hostnameInput = $state('');
  let teamNumberInput = $state<string | number | null>('');
  let identityStatus = $state<string | null>(null);
  let identityBusy = $state(false);
  let identityError = $state<string | null>(null);

  let selectedInterface = $state<string | null>(null);
  let interfaceForm = $state<InterfaceForm | null>(null);
  let interfaceStatus = $state<string | null>(null);
  let interfaceError = $state<string | null>(null);
  let interfaceBusy = $state(false);

  let nt4Enabled = $state(false);
  let nt4SubscriptionsEnabled = $state(true);
  let nt4EmulateLimelightApi = $state(false);
  let nt4EmulatePhotonvisionApi = $state(false);
  let nt4Host = $state('');
  let nt4Port = $state<string | number | null>('5810');
  let nt4PublicApiUrl = $state('');
  let nt4Busy = $state(false);
  let nt4Error = $state<string | null>(null);
  let nt4Status = $state<string | null>(null);

  let nt4ExplorerOpen = $state(false);

  $effect(() => {
    if (deviceState.data) {
      hostnameInput = deviceState.data.hostname;
      teamNumberInput =
        typeof deviceState.data.team_number === 'number' ? String(deviceState.data.team_number) : '';

      const nt4 = deviceState.data.nt4;
      nt4Enabled = Boolean(nt4?.enabled);
      nt4SubscriptionsEnabled = nt4?.subscriptions_enabled ?? true;
      nt4EmulateLimelightApi = nt4?.emulate_limelight_api ?? false;
      nt4EmulatePhotonvisionApi = nt4?.emulate_photonvision_api ?? false;
      nt4Host = nt4?.server_host ?? '';
      nt4Port = typeof nt4?.server_port === 'number' ? String(nt4.server_port) : nt4?.server_port ? String(nt4.server_port) : '5810';
      nt4PublicApiUrl = nt4?.public_api_url ?? '';
    }
  });

  function normalizeText(value: unknown): string {
    if (typeof value === 'string') return value;
    if (typeof value === 'number') return Number.isFinite(value) ? String(value) : '';
    return '';
  }

  function rioServerHostFromTeam(value: unknown): string | null {
    const trimmed = normalizeText(value).trim();
    if (!trimmed) return null;
    const team = Number(trimmed);
    if (!Number.isFinite(team) || !Number.isInteger(team) || team <= 0) return null;
    const a = Math.floor(team / 100);
    const b = team % 100;
    if (a < 0 || a > 255 || b < 0 || b > 255) return null;
    return `10.${a}.${b}.2`;
  }

  const nt4DerivedServerHost = $derived(rioServerHostFromTeam(teamNumberInput));
  const nt4EffectiveServerHost = $derived(nt4Host.trim() || nt4DerivedServerHost || '');
  const nt4PublishPrefix = $derived(hostnameInput.trim() ? `/${hostnameInput.trim()}` : '/helios');

  $effect(() => {
    if (!deviceState.data) {
      interfaceForm = null;
      selectedInterface = null;
      return;
    }
    const interfaces = deviceState.data.interfaces;
    if (
      !selectedInterface ||
      !interfaces.some((iface: DeviceNetworkInterfaceResponse) => iface.name === selectedInterface)
    ) {
      selectedInterface = interfaces[0]?.name ?? null;
    }
    hydrateInterfaceForm(
      interfaces.find((iface: DeviceNetworkInterfaceResponse) => iface.name === selectedInterface) ?? null
    );
  });

  function hydrateInterfaceForm(iface: DeviceNetworkInterfaceResponse | null): void {
    if (!iface) {
      interfaceForm = null;
      return;
    }
    const fallback = iface.dhcp_ipv4 ?? null;
    const staticConfig = iface.static_ipv4 ?? null;
    interfaceForm = {
      name: iface.name,
      mode: iface.mode === 'static' ? 'static' : 'dhcp',
      address: staticConfig?.address ?? fallback?.address ?? '',
      prefix: staticConfig?.prefix ?? fallback?.prefix ?? 24,
      gateway: staticConfig?.gateway ?? fallback?.gateway ?? '',
      leaseLabel: iface.dhcp_ipv4
        ? `${iface.dhcp_ipv4.address}/${iface.dhcp_ipv4.prefix}${
            iface.dhcp_ipv4.gateway ? ` via ${iface.dhcp_ipv4.gateway}` : ''
          }`
        : null,
      mac: iface.mac ?? null
    };
  }

  function selectInterface(name: string): void {
    selectedInterface = name;
    if (!deviceState.data) return;
    const iface =
      deviceState.data.interfaces.find((entry: DeviceNetworkInterfaceResponse) => entry.name === name) ?? null;
    hydrateInterfaceForm(iface);
  }

  async function saveIdentity(): Promise<void> {
    if (!deviceState.data) return;
    const trimmed = hostnameInput.trim();
    if (!trimmed) {
      identityError = 'Hostname is required.';
      return;
    }

    const teamValue = normalizeText(teamNumberInput).trim();
    const parsedTeam = teamValue.length ? Number(teamValue) : null;
    if (parsedTeam !== null && (!Number.isFinite(parsedTeam) || !Number.isInteger(parsedTeam) || parsedTeam <= 0)) {
      identityError = 'Team number must be a positive integer.';
      return;
    }

    identityBusy = true;
    identityError = null;
    identityStatus = null;

    const request: UpdateDeviceSettingsRequest = {
      requested_by: REQUESTED_BY,
      hostname: trimmed,
      team_number: parsedTeam
    };

    try {
      await deviceSettingsStore.patch(request);
      identityStatus = `Persisted at ${new Date().toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })}`;
    } catch (err) {
      identityError = buildErrorMessage({ error: err, fallback: 'Unable to save identity settings.' });
    } finally {
      identityBusy = false;
    }
  }

  async function saveInterface(): Promise<void> {
    if (!interfaceForm) return;

    if (interfaceForm.mode === 'static') {
      if (!interfaceForm.address.trim()) {
        interfaceError = 'Static address is required.';
        return;
      }
      if (!Number.isFinite(interfaceForm.prefix) || interfaceForm.prefix < 1 || interfaceForm.prefix > 32) {
        interfaceError = 'Prefix must be between 1 and 32.';
        return;
      }
      if (!interfaceForm.gateway.trim()) {
        interfaceError = 'Gateway is required for static addressing.';
        return;
      }
    }

    interfaceBusy = true;
    interfaceError = null;
    interfaceStatus = null;

    const request: UpdateDeviceSettingsRequest = {
      requested_by: REQUESTED_BY,
      interfaces: [
        {
          name: interfaceForm.name,
          assignment:
            interfaceForm.mode === 'dhcp'
              ? { mode: 'dhcp' }
              : {
                  mode: 'static',
                  address: interfaceForm.address.trim(),
                  prefix: interfaceForm.prefix,
                  gateway: interfaceForm.gateway.trim() ? interfaceForm.gateway.trim() : null
                }
        }
      ]
    };

    try {
      await deviceSettingsStore.patch(request);
      interfaceStatus = `Applied at ${new Date().toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })}`;
    } catch (err) {
      interfaceError = buildErrorMessage({ error: err, fallback: 'Unable to save interface settings.' });
    } finally {
      interfaceBusy = false;
    }
  }

  async function saveNt4(): Promise<void> {
    nt4Busy = true;
    nt4Error = null;
    nt4Status = null;

    const port = Number(normalizeText(nt4Port).trim() || '5810');
    if (!Number.isFinite(port) || port <= 0 || port > 65535) {
      nt4Error = 'Port must be a valid integer (1-65535).';
      nt4Busy = false;
      return;
    }

    const request: UpdateDeviceSettingsRequest = {
      requested_by: REQUESTED_BY,
      nt4: {
        enabled: nt4Enabled,
        subscriptions_enabled: nt4SubscriptionsEnabled,
        emulate_limelight_api: nt4EmulateLimelightApi,
        emulate_photonvision_api: nt4EmulatePhotonvisionApi,
        server_host: nt4Host.trim() || null,
        server_port: Math.trunc(port),
        public_api_url: nt4PublicApiUrl.trim() || null,
      }
    };

    try {
      await deviceSettingsStore.patch(request);
      nt4Status = `Saved at ${new Date().toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })}`;
    } catch (err) {
      nt4Error = buildErrorMessage({ error: err, fallback: 'Unable to save NT4 settings.' });
    } finally {
      nt4Busy = false;
    }
  }
</script>

<section class="space-y-6">
  <div class="grid gap-6 lg:grid-cols-[minmax(0,0.95fr)_minmax(0,1.15fr)]">
    <section class="space-y-4 rounded border border-surface-800/70 bg-surface-950/30 p-4">
      <header>
        <p class="text-xs uppercase tracking-[0.3em] text-surface-500">Identity</p>
        <p class="text-sm text-surface-400">Update the hostname and team metadata reported to the field.</p>
      </header>

      {#if deviceState.loading && !deviceState.data}
        <p class="text-sm text-surface-500">Loading identity…</p>
      {:else if deviceState.error && !deviceState.data}
        <div class="rounded border border-error-500/40 bg-error-500/10 p-3 text-sm text-error-200">
          <p class="font-semibold">Failed to load device settings.</p>
          <p>{deviceState.error}</p>
          <button
            class="btn btn-xs preset-tonal mt-3 uppercase tracking-[0.3em]"
            onclick={() => deviceSettingsStore.load()}
          >
            Retry
          </button>
        </div>
      {:else if deviceState.initialized && !deviceState.data}
        <div class="rounded border border-surface-800/60 bg-surface-950/30 p-3 text-sm text-surface-400">
          <p class="font-semibold text-surface-200">Device identity unavailable.</p>
          <p class="mt-1 text-xs text-surface-500">No hostname or team number reported by the API.</p>
        </div>
      {:else}
        <form class="space-y-4" onsubmit={(event) => { event.preventDefault(); saveIdentity(); }}>
          <div class="space-y-3">
            <label class="space-y-1 text-sm">
              <span class="text-xs uppercase tracking-[0.3em] text-surface-500">Hostname</span>
              <input class="input w-full" bind:value={hostnameInput} />
            </label>
            <label class="space-y-1 text-sm">
              <span class="text-xs uppercase tracking-[0.3em] text-surface-500">Team number</span>
              <input class="input w-full" type="number" min="1" step="1" bind:value={teamNumberInput} />
            </label>
          </div>
          {#if identityError}
            <p class="text-xs text-error-400">{identityError}</p>
          {/if}
          <div class="flex flex-wrap items-center justify-between gap-3 text-xs text-surface-500">
            <span>{identityStatus ?? 'Last synced via API'}</span>
            <button
              class="btn preset-filled-primary-500 px-4 py-2 text-xs font-semibold uppercase tracking-[0.3em]"
              type="submit"
              disabled={identityBusy || !deviceState.data}
            >
              {identityBusy ? 'Saving…' : 'Persist changes'}
            </button>
          </div>
        </form>
      {/if}
    </section>

    <section class="space-y-4 rounded border border-surface-800/70 bg-surface-950/30 p-4">
      <header class="flex flex-wrap items-start justify-between gap-3">
        <div>
          <p class="text-xs uppercase tracking-[0.3em] text-surface-500">Adapter controls</p>
          <p class="text-sm text-surface-400">Assign DHCP leases or static overrides per interface.</p>
        </div>
        {#if interfaceForm?.mac}
          <span class="text-micro uppercase tracking-[0.35em] text-surface-500">{interfaceForm.mac}</span>
        {/if}
      </header>

      {#if !deviceState.data}
        {#if deviceState.loading}
          <p class="text-sm text-surface-500">Fetching interfaces…</p>
        {:else if deviceState.error}
          <p class="text-sm text-error-400">{deviceState.error}</p>
        {:else}
          <p class="text-sm text-surface-500">No interfaces reported.</p>
        {/if}
      {:else if !interfaceForm}
        <p class="text-sm text-surface-500">Select an interface to manage addressing.</p>
      {:else}
        <div class="space-y-4">
          <label class="space-y-1 text-sm">
            <span class="text-xs uppercase tracking-[0.3em] text-surface-500">Interface</span>
            <select
              class="select w-full"
              bind:value={selectedInterface}
              onchange={(event) => selectInterface((event.target as HTMLSelectElement).value)}
            >
              {#each deviceState.data.interfaces as iface (iface.name)}
                <option value={iface.name}>{iface.name}{iface.mac ? ` · ${iface.mac}` : ''}</option>
              {/each}
            </select>
          </label>

          <div class="grid gap-2 sm:grid-cols-2">
            <button
              type="button"
              class={`rounded border px-3 py-2 text-left text-xs uppercase tracking-[0.25em] transition-colors ${
                interfaceForm.mode === 'dhcp'
                  ? 'border-primary-400 bg-primary-500/20 text-primary-100'
                  : 'border-surface-800 bg-surface-900/40 text-surface-400'
              }`}
              onclick={() => (interfaceForm = interfaceForm && { ...interfaceForm, mode: 'dhcp' })}
            >
              <p>Dynamic (DHCP)</p>
              <p
                class={`mt-1 text-micro normal-case tracking-normal ${
                  interfaceForm.mode === 'dhcp' ? 'text-primary-200' : 'text-surface-500'
                }`}
              >
                Lease from field control
              </p>
            </button>
            <button
              type="button"
              class={`rounded border px-3 py-2 text-left text-xs uppercase tracking-[0.25em] transition-colors ${
                interfaceForm.mode === 'static'
                  ? 'border-primary-400 bg-primary-500/20 text-primary-100'
                  : 'border-surface-800 bg-surface-900/40 text-surface-400'
              }`}
              onclick={() => (interfaceForm = interfaceForm && { ...interfaceForm, mode: 'static' })}
            >
              <p>Static</p>
              <p
                class={`mt-1 text-micro normal-case tracking-normal ${
                  interfaceForm.mode === 'static' ? 'text-primary-200' : 'text-surface-500'
                }`}
              >
                Manual override until cleared
              </p>
            </button>
          </div>

          <div class="grid gap-3 lg:grid-cols-3">
            <label class="space-y-1 text-sm lg:col-span-2">
              <span class="text-xs uppercase tracking-[0.3em] text-surface-500">Address</span>
              <input
                class={`input w-full transition-opacity ${interfaceForm.mode === 'dhcp' ? 'opacity-50' : ''}`}
                bind:value={interfaceForm.address}
                disabled={interfaceForm.mode === 'dhcp'}
              />
            </label>
            <label class="space-y-1 text-sm">
              <span class="text-xs uppercase tracking-[0.3em] text-surface-500">Prefix</span>
              <input
                class={`input w-full transition-opacity ${interfaceForm.mode === 'dhcp' ? 'opacity-50' : ''}`}
                type="number"
                min="1"
                max="32"
                bind:value={interfaceForm.prefix}
                disabled={interfaceForm.mode === 'dhcp'}
              />
            </label>
          </div>
          <label class="space-y-1 text-sm">
            <span class="text-xs uppercase tracking-[0.3em] text-surface-500">Gateway</span>
            <input
              class={`input w-full transition-opacity ${interfaceForm.mode === 'dhcp' ? 'opacity-50' : ''}`}
              bind:value={interfaceForm.gateway}
              disabled={interfaceForm.mode === 'dhcp'}
            />
          </label>

          {#if interfaceForm.mode === 'dhcp' && interfaceForm.leaseLabel}
            <p class="text-xs text-surface-500">Current lease · {interfaceForm.leaseLabel}</p>
          {/if}
          {#if interfaceError}
            <p class="text-xs text-error-400">{interfaceError}</p>
          {/if}
          <div class="flex flex-wrap items-center justify-between gap-3 text-xs text-surface-500">
            <span>{interfaceStatus ?? 'Applies via POST /device/network'}</span>
            <button
              class="btn preset-filled-primary-500 px-4 py-2 text-xs font-semibold uppercase tracking-[0.3em]"
              type="button"
              onclick={() => saveInterface()}
              disabled={interfaceBusy}
            >
              {interfaceBusy ? 'Applying…' : interfaceForm.mode === 'dhcp' ? 'Request lease' : 'Apply static'}
            </button>
          </div>
        </div>
      {/if}
    </section>
  </div>

  <section class="space-y-4 rounded border border-surface-800/70 bg-surface-950/30 p-4">
    <header>
      <p class="text-xs uppercase tracking-[0.3em] text-surface-500">NetworkTables (NT4)</p>
      <p class="text-sm text-surface-400">Configure an NT4 server target and publish basic HeliOS info.</p>
    </header>

    <form class="space-y-4" onsubmit={(event) => { event.preventDefault(); saveNt4(); }}>
      <label class="flex items-center gap-3 text-sm">
        <input class="checkbox" type="checkbox" bind:checked={nt4Enabled} />
        <span class="text-xs uppercase tracking-[0.3em] text-surface-500">Enable NT4 publishing</span>
      </label>
      <label class="flex items-center gap-3 text-sm">
        <input class="checkbox" type="checkbox" bind:checked={nt4SubscriptionsEnabled} />
        <span class="text-xs uppercase tracking-[0.3em] text-surface-500">Enable NT4 subscriptions</span>
      </label>
      <label class="flex items-center gap-3 text-sm">
        <input class="checkbox" type="checkbox" bind:checked={nt4EmulateLimelightApi} />
        <span class="text-xs uppercase tracking-[0.3em] text-surface-500">Emulate Limelight API (per stream)</span>
      </label>
      <label class="flex items-center gap-3 text-sm">
        <input class="checkbox" type="checkbox" bind:checked={nt4EmulatePhotonvisionApi} />
        <span class="text-xs uppercase tracking-[0.3em] text-surface-500">Emulate PhotonVision API (device-wide)</span>
      </label>

      <div class="grid gap-3 md:grid-cols-2">
        <label class="space-y-1 text-sm">
          <span class="text-xs uppercase tracking-[0.3em] text-surface-500">Server host override</span>
          <input class="input w-full" placeholder={nt4DerivedServerHost ?? '10.xx.yy.2'} bind:value={nt4Host} />
          <p class="text-xs text-surface-500">
            Default: {nt4DerivedServerHost ?? 'Set a team number to derive the roboRIO address.'}
          </p>
        </label>
        <label class="space-y-1 text-sm">
          <span class="text-xs uppercase tracking-[0.3em] text-surface-500">Server port</span>
          <input class="input w-full" type="number" min="1" max="65535" step="1" bind:value={nt4Port} />
        </label>
      </div>

      <div class="grid gap-3 md:grid-cols-2">
        <div class="space-y-1 text-sm">
          <span class="text-xs uppercase tracking-[0.3em] text-surface-500">Publish prefix</span>
          <p class="text-xs text-surface-500">
            <span class="font-mono">{nt4PublishPrefix}</span> (derived from hostname)
          </p>
        </div>
        <div class="space-y-1 text-sm">
          <span class="text-xs uppercase tracking-[0.3em] text-surface-500">Published topics</span>
          <p class="text-xs text-surface-500">
            <span class="font-mono">{nt4PublishPrefix}</span>/info/hostname · api_url · ui_url · streams
          </p>
        </div>
      </div>

      <div class="grid gap-3 md:grid-cols-2">
        <label class="space-y-1 text-sm">
          <span class="text-xs uppercase tracking-[0.3em] text-surface-500">Public API URL (optional)</span>
          <input class="input w-full" placeholder="http://helios.local:5800" bind:value={nt4PublicApiUrl} />
        </label>
        <div class="space-y-1 text-sm">
          <span class="text-xs uppercase tracking-[0.3em] text-surface-500">Public UI URL</span>
          <p class="text-xs text-surface-500">Published automatically as the device IP + API port.</p>
        </div>
      </div>

      {#if nt4Error}
        <p class="text-xs text-error-400">{nt4Error}</p>
      {:else if nt4Status}
        <p class="text-xs text-success-400">{nt4Status}</p>
      {/if}

      <div class="flex flex-wrap items-center justify-between gap-3">
        <button class="btn preset-filled-primary-500 px-4 py-2 text-xs font-semibold uppercase tracking-[0.3em]" type="submit" disabled={nt4Busy}>
          {nt4Busy ? 'Saving…' : 'Save NT4'}
        </button>
        {#if NT4_EXPLORER_VISIBLE}
          <button
            class="btn preset-tonal px-4 py-2 text-xs font-semibold uppercase tracking-[0.3em]"
            type="button"
            onclick={() => (nt4ExplorerOpen = true)}
            disabled={nt4Busy}
            title={!nt4SubscriptionsEnabled ? 'Enable NT4 subscriptions to browse live topics.' : !nt4EffectiveServerHost.trim() ? 'Set a team number or server host override.' : 'Browse live NT4 topics.'}
          >
            Open Explorer
          </button>
        {/if}
      </div>
    </form>
  </section>
</section>

{#if NT4_EXPLORER_VISIBLE}
  <Nt4ExplorerModal
    open={nt4ExplorerOpen}
    onClose={() => (nt4ExplorerOpen = false)}
    targetHost={nt4EffectiveServerHost}
    targetPort={Number(normalizeText(nt4Port).trim() || '5810')}
    subscriptionsEnabled={nt4SubscriptionsEnabled}
  />
{/if}
