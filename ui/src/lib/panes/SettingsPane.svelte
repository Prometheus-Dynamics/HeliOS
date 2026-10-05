<script lang="ts">
  // Look and feel, names and colours for everything, layouts, and backups.
  import Icon from "$lib/components/common/Icon.svelte";
  import { exportAll, importAll } from "$lib/core/backup";
  import { colorVar, identity } from "$lib/core/identity.svelte";
  import { prefs, THEMES } from "$lib/core/prefs.svelte";
  import { shell } from "$lib/core/shell.svelte";
  import { workspaces } from "$lib/core/workspace.svelte";
  import { downloadJson, pickJson } from "$lib/files";
  import IconButton from "$lib/kit/IconButton.svelte";
  import Prop from "$lib/kit/Prop.svelte";
  import Section from "$lib/kit/Section.svelte";
  import Seg from "$lib/kit/Seg.svelte";
  import Swatch from "$lib/kit/Swatch.svelte";
  import IdentityEditor from "$lib/kit/IdentityEditor.svelte";
  import { cluster } from "$lib/stores/cluster.svelte";
  import { exportWorkspace, importWorkspace } from "$lib/shell/workspace-actions";
  import Num from "$lib/kit/Num.svelte";
  import { team } from "$lib/core/team.svelte";
  import { gallery } from "$lib/shell/gallery.svelte";
  import { help } from "$lib/shell/help.svelte";

  const ACCENTS = ["#ff6b6b", "#ff7a3d", "#f5a524", "#3ccf7e", "#2dd4bf", "#4c8dff", "#9775fa", "#f783ac"];
  let identityKind = $state<"cameras" | "pipelines" | "devices">("cameras");
  const objects = $derived(
    identityKind === "cameras"
      ? cluster.cameras.map((c) => ({ id: c.resourceId, name: c.name, icon: "camera" as const, sub: c.mount }))
      : identityKind === "pipelines"
        ? cluster.workloads.map((w) => ({ id: w.id, name: w.name, icon: "schema" as const, sub: cluster.node(w.nodeId)?.name ?? "" }))
        : cluster.nodes.map((n) => ({ id: n.id, name: n.name, icon: "cpu" as const, sub: n.model })),
  );
  const SHORTCUTS = [
    ["Ctrl K", "Search and run anything"],
    ["1 – 9", "Switch screen"],
    ["Space", "Pause or resume live feeds"],
    ["Ctrl S", "Deploy the pipeline you are editing"],
    ["Ctrl Z / Ctrl Shift Z", "Undo / redo in the graph"],
    ["Ctrl L", "Tidy the graph layout"],
    ["Double-click a tab row", "Maximize that pane"],
    ["Drag a tab", "Move it, or drop on an edge to split"],
    ["Right-click", "Actions for cameras, devices, processes, tabs, screens"],
  ];
</script>

<div class="settings">
  <Section title="Robot" key="set-robot">
    <Prop label="Team number" help="Sets where robot code is expected (NetworkTables server).">
      <Num value={team.number ?? 0} min={0} max={99999} width={90} label="Team number" onchange={(v) => (team.number = v || null)} />
    </Prop>
    <Prop label="Robot code at"><span class="mono">{team.ntServer}</span></Prop>
    <Prop label="Screens in the rail">
      <button type="button" class="mini" onclick={() => gallery.show("screens")}>Choose…</button>
      <button type="button" class="mini" onclick={() => workspaces.setVisible(null)}>Show all</button>
    </Prop>
    <Prop label="Help">
      <button type="button" class="mini" onclick={() => help.startTour()}>Tour</button>
      <button type="button" class="mini" onclick={() => (help.onboarding = true)}>First-time setup</button>
    </Prop>
  </Section>

  <Section title="Theme" key="set-theme">
    <div class="themes">
      {#each THEMES as t (t.id)}
        <button type="button" class="theme" class:on={prefs.theme === t.id} data-theme={t.id} onclick={() => (prefs.theme = t.id)}>
          <span class="mini">
            <span class="mrail"></span>
            <span class="mbody">
              <span class="mtabs"><i></i><i></i></span>
              <span class="mpane"><b></b><em></em><em></em><u></u></span>
            </span>
          </span>
          <span class="tname">{t.name}</span>
          <span class="tnote">{t.note}</span>
        </button>
      {/each}
    </div>
    <Prop label="Density"><Seg label="Density" value={prefs.density} options={[{ value: "compact", label: "Compact" }, { value: "comfortable", label: "Comfortable" }]} onchange={(v) => (prefs.density = v)} /></Prop>
    <Prop label="Accent">
      <span class="accents">
        <button type="button" class="acc auto" class:on={!prefs.accent} onclick={() => (prefs.accent = null)} data-tip="Theme default">A</button>
        {#each ACCENTS as a (a)}
          <button type="button" class="acc" class:on={prefs.accent === a} style:background={a} aria-label="Accent {a}" onclick={() => (prefs.accent = a)}></button>
        {/each}
        <input type="color" value={prefs.accent ?? "#ff6b6b"} aria-label="Custom accent" oninput={(e) => (prefs.accent = (e.currentTarget as HTMLInputElement).value)} />
      </span>
    </Prop>
    <p class="hint">A theme is one block of colour tokens in <code>src/themes/themes.css</code>; copy one, rename it, and add it to the list in <code>prefs.svelte.ts</code>.</p>
  </Section>

  <Section title="Names, icons and colours" key="set-ident">
    {#snippet actions()}
      <IconButton icon="file-export" label="Export identities" size={22} onclick={() => downloadJson("helios-identities.json", { format: "helios.identities", schema_version: 1, identities: identity.export() })} />
      <IconButton icon="file-import" label="Import identities" size={22} onclick={async () => { const d = await pickJson<{ identities?: Record<string, never> }>(); if (d?.identities) identity.import(d.identities); }} />
    {/snippet}
    <div class="seg"><Seg label="Objects" value={identityKind} options={[{ value: "cameras", label: "Cameras" }, { value: "pipelines", label: "Pipelines" }, { value: "devices", label: "Devices" }]} onchange={(v) => (identityKind = v)} /></div>
    <div class="idents">
      {#each objects as o (o.id)}
        <div class="ident"><IdentityEditor id={o.id} fallbackName={o.name} fallbackIcon={o.icon} sub={o.sub} /></div>
      {/each}
    </div>
  </Section>

  <Section title="Screens and layouts" key="set-layouts" count={workspaces.all.length}>
    {#snippet actions()}
      <IconButton icon="file-import" label="Import a layout" size={22} onclick={importWorkspace} />
    {/snippet}
    {#each workspaces.all as w (w.id)}
      <div class="ws" style:--c={colorVar(w.color)}>
        <span class="wsi"><Icon name={w.icon} size={14} /></span>
        <button type="button" class="wsn" onclick={() => shell.go(w.id)}>{w.name}</button>
        {#if workspaces.isModified(w.id)}<span class="mod">edited</span>{/if}
        {#if !w.builtin}<span class="mod">yours</span>{/if}
        <IconButton icon="file-export" label="Export layout" size={22} onclick={() => exportWorkspace(w.id)} />
        {#if w.builtin}
          <IconButton icon="rotate-clockwise" label="Reset layout" size={22} disabled={!workspaces.isModified(w.id)} onclick={() => workspaces.reset(w.id)} />
        {:else}
          <IconButton icon="trash" label="Delete" tone="danger" size={22} onclick={() => workspaces.remove(w.id)} />
        {/if}
      </div>
    {/each}
  </Section>

  <Section title="Backup" key="set-backup">
    <p class="hint">Everything set up here (look, names, layouts, camera mounts, field, calibrations, camera settings) in one file. Restore it on another laptop or after a reflash.</p>
    <div class="row">
      <button type="button" class="btn" onclick={() => downloadJson(`helios-config-${new Date().toISOString().slice(0, 10)}.json`, exportAll())}><Icon name="download" size={13} /> Export everything</button>
      <button type="button" class="btn ghost" onclick={async () => importAll(await pickJson())}><Icon name="upload" size={13} /> Restore from file…</button>
    </div>
  </Section>

  <Section title="Keyboard and mouse" key="set-keys" open={false}>
    {#each SHORTCUTS as [k, d] (k)}
      <Prop label={d}><kbd>{k}</kbd></Prop>
    {/each}
  </Section>
</div>

<style>
  .settings {
    flex: 1;
    overflow-y: auto;
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(440px, 1fr));
    align-content: start;
    column-gap: 1px;
    background: var(--line);
  }
  .settings > :global(section) {
    background: var(--s1);
  }
  .themes {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(150px, 1fr));
    gap: 8px;
    padding: 2px 10px 8px;
  }
  .theme {
    display: flex;
    flex-direction: column;
    gap: 1px;
    padding: 6px;
    text-align: left;
    color: var(--fg);
    background: var(--s0);
    border: 1px solid var(--line);
    border-radius: var(--r-3);
    font-size: var(--font-size, 13px);
  }
  .theme.on {
    outline: 2px solid var(--accent);
    outline-offset: 1px;
  }
  .mini {
    display: flex;
    height: 64px;
    margin-bottom: 5px;
    border-radius: var(--r-2);
    overflow: hidden;
    background: var(--s0);
    border: 1px solid var(--line);
  }
  .mrail {
    width: 12px;
    background: var(--s0);
    border-right: 1px solid var(--line);
  }
  .mbody {
    flex: 1;
    display: flex;
    flex-direction: column;
    padding: 4px;
    gap: 3px;
  }
  .mtabs {
    display: flex;
    gap: 3px;
  }
  .mtabs i {
    width: 22px;
    height: 6px;
    background: var(--s2);
    border-top: 2px solid var(--accent);
    border-radius: 1px;
  }
  .mtabs i + i {
    border-top-color: var(--line-strong);
  }
  .mpane {
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: 3px;
    padding: 4px;
    background: var(--s1);
    border: 1px solid var(--line);
    border-radius: var(--r-1);
  }
  .mpane b {
    width: 60%;
    height: 5px;
    background: var(--fg);
    border-radius: 2px;
    opacity: 0.8;
  }
  .mpane em {
    width: 80%;
    height: 3px;
    background: var(--fg-3);
    border-radius: 2px;
  }
  .mpane u {
    width: 34px;
    height: 8px;
    margin-top: auto;
    background: var(--accent);
    border-radius: var(--r-1);
  }
  .tname {
    font-size: 12px;
    font-weight: 650;
  }
  .tnote {
    font-size: 10.5px;
    color: var(--fg-3);
  }
  .accents {
    display: flex;
    align-items: center;
    gap: 4px;
  }
  .acc {
    width: 18px;
    height: 18px;
    border-radius: 50%;
  }
  .acc.on {
    box-shadow: 0 0 0 2px var(--s1), 0 0 0 4px var(--fg);
  }
  .acc.auto {
    font-size: 10px;
    font-weight: 700;
    color: var(--fg-2);
    border: 1px solid var(--line-strong);
  }
  .accents input {
    width: 22px;
    height: 20px;
    padding: 0;
    border: 0;
    background: none;
  }
  .hint {
    margin: 4px 10px 8px;
    font-size: 11.5px;
    color: var(--fg-3);
    line-height: 1.5;
  }
  code {
    font-family: var(--font-code);
    font-size: 11px;
    color: var(--fg-2);
  }
  .seg {
    padding: 0 10px 4px;
  }
  .idents {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(240px, 1fr));
  }
  .ws {
    display: flex;
    align-items: center;
    gap: 8px;
    height: 30px;
    padding: 0 8px 0 10px;
  }
  .ws:hover {
    background: var(--s2);
  }
  .wsi {
    display: grid;
    place-items: center;
    width: 22px;
    height: 22px;
    border-radius: var(--r-1);
    color: var(--c);
    background: color-mix(in oklab, var(--c) 16%, transparent);
  }
  .wsn {
    flex: 1;
    text-align: left;
    font-size: 12.5px;
    font-weight: 500;
  }
  .mod {
    font-size: 10.5px;
    color: var(--fg-3);
  }
  .row {
    display: flex;
    gap: 6px;
    padding: 0 10px 6px;
  }
  .btn {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: 28px;
    padding: 0 12px;
    font-size: 12px;
    font-weight: 600;
    color: var(--on-accent);
    background: var(--accent);
    border-radius: var(--r-2);
  }
  .btn.ghost {
    color: var(--fg);
    background: transparent;
    border: 1px solid var(--line-strong);
  }
  .mini {
    height: 22px;
    padding: 0 9px;
    font-size: 11.5px;
    font-weight: 600;
    color: var(--fg-2);
    border: 1px solid var(--line-strong);
    border-radius: var(--r-1);
  }
  .mini:hover {
    color: var(--fg);
    background: var(--s3);
  }
  .mono {
    font-family: var(--font-code);
    font-size: 11.5px;
  }
  kbd {
    font-family: var(--font-code);
    font-size: 11px;
    padding: 1px 6px;
    color: var(--fg-2);
    border: 1px solid var(--line-strong);
    border-radius: var(--r-1);
  }
</style>
