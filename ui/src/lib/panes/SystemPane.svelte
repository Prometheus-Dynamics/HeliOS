<script lang="ts">
  // One device's software: boot slots and updates, services, recovery
  // guarantees, and its whole configuration as a file.
  import Icon from "$lib/components/common/Icon.svelte";
  import { duration } from "$lib/format";
  import { identity } from "$lib/core/identity.svelte";
  import { selection } from "$lib/core/selection.svelte";
  import { RECOVERY } from "$lib/devices";
  import { downloadJson, pickJson } from "$lib/files";
  import Badge from "$lib/kit/Badge.svelte";
  import IconButton from "$lib/kit/IconButton.svelte";
  import Picker from "$lib/kit/Picker.svelte";
  import Prop from "$lib/kit/Prop.svelte";
  import Section from "$lib/kit/Section.svelte";
  import { cluster } from "$lib/stores/cluster.svelte";
  import { system } from "$lib/stores/system.svelte";
  import { toasts } from "$lib/stores/toasts.svelte";
  import PaneBar from "$lib/workspace/PaneBar.svelte";
  import type { PaneProps } from "$lib/workspace/panes";
  import { follow } from "./follow.svelte";

  let { pane, ws }: PaneProps = $props();
  const target = follow(() => pane, () => ws, "device", () => cluster.nodes[0]?.id);
  const node = $derived(target.id ? cluster.node(target.id) : undefined);
  const available = $derived(node?.kind === "raze" ? "2026.5.0" : node?.kind === "mcu" ? "0.3.1" : null);
  let confirmSafe = $state(false);

  function exportConfig() {
    if (!node) return;
    const hw = system.of(node.id);
    downloadJson(`${node.id}.device.json`, {
      format: "helios.device",
      schema_version: 1,
      device: node.id,
      identity: identity.get(node.id),
      cameras: cluster.cameras.filter((c) => c.nodeId === node.id).map((c) => ({ id: c.resourceId, settings: c.settings, extra: c.extra ?? {}, identity: identity.get(c.resourceId) })),
      pipelines: cluster.workloadsOn(node.id).map((w) => ({ id: w.id, name: w.name, bindings: w.bindings, graph: w.graph })),
      hardware: hw ? { fan: hw.fan, leds: hw.leds, gpio: hw.gpio.filter((g) => !g.owner) } : null,
    });
  }

  async function importConfig() {
    if (!node) return;
    const doc = await pickJson<{ format?: string; cameras?: { id: string; settings: never; extra?: never }[] }>();
    if (doc?.format !== "helios.device") return toasts.error("That file is not a HeliOS device config");
    for (const c of doc.cameras ?? []) if (cluster.camera(c.id)) cluster.setCamera(c.id, c.settings, c.extra);
    toasts.success(`Applied ${doc.cameras?.length ?? 0} camera configs to ${node.name}. Pipelines are offered in the Pipelines screen.`);
  }
</script>

<PaneBar>
  <Picker
    label="Device"
    icon="cpu"
    value={node?.id}
    options={cluster.nodes.map((n) => ({ id: n.id, name: identity.name(n.id, n.name) }))}
    onpick={(id) => (target.pinned ? target.pin(id) : selection.select({ kind: "device", id }))}
    pinned={target.pinned}
    ontogglepin={() => target.toggle()}
  />
  <span class="sep"></span>
  <IconButton icon="file-import" label="Apply a device config" size={24} onclick={importConfig} />
  <IconButton icon="file-export" label="Export device config" size={24} onclick={exportConfig} />
</PaneBar>

{#if node}
  <div class="scroll">
    <div class="top">
      <div class="os">
        <b>{node.os.name} {node.os.version}</b>
        <span>agent {node.agent.version} · up {duration(node.uptimeS)}</span>
      </div>
      <IconButton icon="power" label="Reboot" text="Reboot" size={26} onclick={() => cluster.reboot(node.id)} />
      {#if confirmSafe}
        <IconButton icon="lifebuoy" label="Confirm safe mode" text="Stop apps?" tone="accent" size={26} onclick={() => { confirmSafe = false; cluster.safeMode(node.id); }} />
      {:else}
        <IconButton icon="lifebuoy" label="Safe mode: stop applications, keep the agent and control plane" text="Safe mode" size={26} onclick={() => { confirmSafe = true; setTimeout(() => (confirmSafe = false), 3000); }} />
      {/if}
    </div>

    {#if node.slots.length}
      <Section title="Boot slots" key="sys-slots">
        <div class="slots">
          {#each node.slots as s (s.name)}
            <div class="slot" class:active={s.active}>
              <span class="sn">{s.name}</span>
              <span class="sv">
                <b>{s.version}</b>
                <i>{s.active ? "running" : "spare"} · {s.confirmed ? "confirmed" : "on trial"}</i>
              </span>
              {#if s.active}<Badge tone="ok" text="active" />{:else}<IconButton icon="versions" label="Boot into slot {s.name}" size={22} onclick={() => cluster.switchSlot(node.id)} />{/if}
            </div>
          {/each}
        </div>
        {#if available && available !== node.os.version}
          <div class="update">
            <Icon name="cloud-download" size={14} />
            <span><b>{available}</b> is available. It is written to the spare slot and boots on trial; a failed health check falls back by itself.</span>
            <IconButton icon="download" label="Install {available}" text="Install" tone="accent" size={24} onclick={() => cluster.installUpdate(node.id, available)} />
          </div>
        {/if}
      </Section>
    {/if}

    <Section title="Services" key="sys-services" count={node.services.length}>
      {#each node.services as s (s.name)}
        <div class="svc">
          <span class="dot s-{s.state}"></span>
          <span class="nm">{s.name}</span>
          <span class="mono dim">{s.memMiB < 1 ? `${(s.memMiB * 1024).toFixed(0)} KiB` : `${s.memMiB.toFixed(1)} MiB`}</span>
          <span class="mono dim" data-tip="Restarts since boot">↻{s.restarts}</span>
          <IconButton icon="refresh" label="Restart {s.name}" size={20} onclick={() => cluster.restartService(node.id, s.name)} />
        </div>
      {/each}
    </Section>

    <Section title="Recovery" key="sys-recovery">
      {#each Object.entries(RECOVERY) as [level, r] (level)}
        {@const ok = node.recovery.includes(level as never)}
        <div class="rec" class:ok>
          <span class="lv">{level}</span>
          <span class="rt"><b>{r.title}</b><i>{r.detail}</i></span>
          <Icon name={ok ? "circle-check" : "circle-dashed"} size={15} />
        </div>
      {/each}
    </Section>

    <Section title="Clock and link" key="sys-clock" advanced>
      <Prop label="Clock source"><span class="mono">{node.clock.source}</span></Prop>
      <Prop label="Offset"><span class="mono">{node.clock.offsetUs} µs</span></Prop>
      <Prop label="Link"><span class="mono">{node.link} · {node.address}</span></Prop>
      <Prop label="Disk"><span class="mono">{node.diskUsedMiB.toFixed(1)} / {node.diskTotalMiB} MiB</span></Prop>
    </Section>
  </div>
{/if}

<style>
  .scroll {
    flex: 1;
    overflow-y: auto;
  }
  .top {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 8px 10px;
    border-bottom: 1px solid var(--line);
  }
  .os {
    flex: 1;
    display: flex;
    flex-direction: column;
    min-width: 0;
  }
  .os b {
    font-size: 13.5px;
  }
  .os span {
    font-size: 11px;
    color: var(--fg-3);
  }
  .slots {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 6px;
    padding: 0 10px 6px;
  }
  .slot {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 6px 8px;
    border: 1px solid var(--line);
    border-radius: var(--r-2);
  }
  .slot.active {
    border-color: color-mix(in oklab, var(--ok) 40%, var(--line));
    background: color-mix(in oklab, var(--ok) 6%, transparent);
  }
  .sn {
    display: grid;
    place-items: center;
    width: 22px;
    height: 22px;
    font-weight: 700;
    font-size: 12px;
    border-radius: var(--r-1);
    background: var(--s3);
  }
  .sv {
    flex: 1;
    display: flex;
    flex-direction: column;
    min-width: 0;
  }
  .sv b {
    font-family: var(--font-code);
    font-size: 12px;
  }
  .sv i {
    font-style: normal;
    font-size: 10.5px;
    color: var(--fg-3);
  }
  .update {
    display: flex;
    align-items: center;
    gap: 8px;
    margin: 0 10px 6px;
    padding: 6px 8px;
    font-size: 11.5px;
    color: var(--fg-2);
    background: var(--info-bg);
    border-radius: var(--r-2);
  }
  .update :global(svg) {
    color: var(--info);
  }
  .svc {
    display: flex;
    align-items: center;
    gap: 8px;
    height: 26px;
    padding: 0 10px;
    font-size: 12px;
  }
  .svc:hover {
    background: color-mix(in oklab, var(--s3) 50%, transparent);
  }
  .nm {
    flex: 1;
  }
  .dot {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--fg-4);
  }
  .s-running {
    background: var(--ok);
  }
  .s-restarting {
    background: var(--info);
  }
  .s-failed {
    background: var(--err);
  }
  .mono {
    font-family: var(--font-code);
    font-size: 11px;
  }
  .dim {
    color: var(--fg-3);
  }
  .rec {
    display: flex;
    align-items: flex-start;
    gap: 8px;
    padding: 5px 10px;
    color: var(--fg-4);
  }
  .rec.ok {
    color: var(--ok);
  }
  .lv {
    font-family: var(--font-code);
    font-size: 11px;
    font-weight: 600;
    padding-top: 1px;
  }
  .rt {
    flex: 1;
    display: flex;
    flex-direction: column;
  }
  .rt b {
    font-size: 12px;
    font-weight: 600;
    color: var(--fg);
  }
  .rec:not(.ok) .rt b {
    color: var(--fg-3);
  }
  .rt i {
    font-style: normal;
    font-size: 11px;
    color: var(--fg-3);
    line-height: 1.4;
  }
</style>
