<script lang="ts">
  // The field from above: drag tags to move them, drag the arrow to turn
  // them, double-click empty floor to add one. The robot and its cameras'
  // fields of view show what it can see from where it stands (drag it too).
  import { colorVar, identity } from "$lib/core/identity.svelte";
  import { selection } from "$lib/core/selection.svelte";
  import { workspaces } from "$lib/core/workspace.svelte";
  import { downloadJson, pickJson } from "$lib/files";
  import { menu } from "$lib/core/menu.svelte";
  import IconButton from "$lib/kit/IconButton.svelte";
  import { cluster } from "$lib/stores/cluster.svelte";
  import { field } from "$lib/stores/field.svelte";
  import { robot } from "$lib/stores/robot.svelte";
  import { CAMERA_HFOV_DEG } from "$lib/three/mounts";
  import PaneBar from "$lib/workspace/PaneBar.svelte";
  import type { PaneProps } from "$lib/workspace/panes";

  let { pane, ws }: PaneProps = $props();
  const snap = $derived((pane.props?.snap as boolean) ?? true);
  const showFov = $derived((pane.props?.fov as boolean) ?? true);
  const L = $derived(field.layout.length);
  const W = $derived(field.layout.width);
  const PAD = 0.4;
  const selTag = $derived(selection.current?.kind === "tag" && !selection.current.parent ? Number(selection.current.id) : null);

  let svg = $state<SVGSVGElement>();
  let drag = $state<{ kind: "tag" | "turn" | "robot" | "robot-turn"; id?: number } | null>(null);

  // Field coordinates: x right, y up (WPILib). SVG y is flipped.
  function toField(e: PointerEvent | MouseEvent) {
    const pt = svg!.createSVGPoint();
    pt.x = e.clientX;
    pt.y = e.clientY;
    const p = pt.matrixTransform(svg!.getScreenCTM()!.inverse());
    return { x: p.x, y: W - p.y };
  }
  const q = (v: number) => (snap ? Math.round(v / 0.05) * 0.05 : v);

  function move(e: PointerEvent) {
    if (!drag) return;
    const p = toField(e);
    if (drag.kind === "tag" && drag.id !== undefined) field.update(drag.id, { x: +q(Math.max(0, Math.min(L, p.x))).toFixed(3), y: +q(Math.max(0, Math.min(W, p.y))).toFixed(3) });
    else if (drag.kind === "turn" && drag.id !== undefined) {
      const t = field.layout.tags.find((x) => x.id === drag!.id)!;
      let yaw = (Math.atan2(p.y - t.y, p.x - t.x) * 180) / Math.PI;
      if (snap) yaw = Math.round(yaw / 15) * 15;
      field.update(drag.id, { yaw: +yaw.toFixed(1) });
    } else if (drag.kind === "robot") {
      field.robotPose.x = Math.max(0, Math.min(L, p.x));
      field.robotPose.y = Math.max(0, Math.min(W, p.y));
    } else if (drag.kind === "robot-turn") {
      field.robotPose.heading = (Math.atan2(p.y - field.robotPose.y, p.x - field.robotPose.x) * 180) / Math.PI;
    }
  }

  const fovs = $derived.by(() => {
    const out: { id: string; color: string; d: string }[] = [];
    const r = field.robotPose;
    const h = (r.heading * Math.PI) / 180;
    for (const [id, p] of Object.entries(robot.placements)) {
      const cx = r.x + p.pos[0] * Math.cos(h) - p.pos[1] * Math.sin(h);
      const cy = r.y + p.pos[0] * Math.sin(h) + p.pos[1] * Math.cos(h);
      const a = h + (p.yaw * Math.PI) / 180;
      const half = (CAMERA_HFOV_DEG / 2) * (Math.PI / 180);
      const R = 5;
      const pts = [[cx, cy], [cx + R * Math.cos(a - half), cy + R * Math.sin(a - half)], [cx + R * Math.cos(a + half), cy + R * Math.sin(a + half)]];
      out.push({ id, color: colorVar(identity.get(id).color), d: `M${pts.map(([x, y]) => `${x},${W - y}`).join(" L")}Z` });
    }
    return out;
  });

  // Which tags each camera could see from here (in range and in the wedge).
  const visible = $derived.by(() => {
    const set = new Set<number>();
    const r = field.robotPose;
    const h = (r.heading * Math.PI) / 180;
    for (const p of Object.values(robot.placements)) {
      const a = h + (p.yaw * Math.PI) / 180;
      for (const t of field.layout.tags) {
        const dx = t.x - r.x;
        const dy = t.y - r.y;
        const dist = Math.hypot(dx, dy);
        let off = Math.atan2(dy, dx) - a;
        off = Math.atan2(Math.sin(off), Math.cos(off));
        const facing = Math.cos(((t.yaw * Math.PI) / 180) - Math.atan2(-dy, -dx)) > 0.2;
        if (dist < 5 && Math.abs(off) < (CAMERA_HFOV_DEG / 2) * (Math.PI / 180) && facing) set.add(t.id);
      }
    }
    return set;
  });

  async function importLayout() {
    const doc = await pickJson<unknown>();
    if (doc) field.fromWpilib(doc, "Imported layout");
  }
</script>

<PaneBar>
  <span class="count">{field.layout.tags.length} tags · {L.toFixed(2)} × {W.toFixed(2)} m</span>
  <span class="sep"></span>
  <IconButton icon="grid-4x4" label={snap ? "Snapping to 5 cm / 15°" : "Free placement"} active={snap} size={24} onclick={() => workspaces.setProps(ws, pane.id, { snap: !snap })} />
  <IconButton icon="eye" label="Camera fields of view" active={showFov} size={24} onclick={() => workspaces.setProps(ws, pane.id, { fov: !showFov })} />
  <IconButton icon="file-import" label="Import WPILib field layout" size={24} onclick={importLayout} />
  <IconButton icon="file-export" label="Export WPILib field layout" size={24} onclick={() => downloadJson(`${field.layout.name.toLowerCase().replace(/\W+/g, "-")}.json`, field.toWpilib())} />
  <IconButton icon="dots-vertical" label="More" size={24} onclick={(e) => menu.below(e.currentTarget as Element, [{ label: "Reset to demo field", icon: "rotate-clockwise", run: () => field.resetDemo() }], "end")} />
</PaneBar>

<div class="wrap">
  <svg
    bind:this={svg}
    viewBox="{-PAD} {-PAD} {L + PAD * 2} {W + PAD * 2}"
    role="application"
    aria-label="Field editor"
    onpointermove={move}
    onpointerup={() => (drag = null)}
    ondblclick={(e) => {
      if ((e.target as Element).closest(".tag, .robot")) return;
      const p = toField(e);
      const id = field.add(q(p.x), q(p.y));
      selection.select({ kind: "tag", id: String(id) });
    }}
  >
    <defs>
      <pattern id="fgrid" width="1" height="1" patternUnits="userSpaceOnUse"><path d="M1,0 L0,0 0,1" fill="none" class="gl" /></pattern>
    </defs>
    <rect x="0" y="0" width={L} height={W} class="floor" />
    <rect x="0" y="0" width={L} height={W} fill="url(#fgrid)" />
    <line x1={L / 2} y1="0" x2={L / 2} y2={W} class="mid" />
    <rect x="0" y="0" width={L} height={W} class="wall" />
    {#if showFov}
      {#each fovs as f (f.id)}<path d={f.d} fill={f.color} class="fov" />{/each}
    {/if}
    {#each field.layout.tags as t (t.id)}
      {@const sel = selTag === t.id}
      {@const a = (t.yaw * Math.PI) / 180}
      <g class="tag" class:sel class:seen={visible.has(t.id)} transform="translate({t.x},{W - t.y})">
        <line x1="0" y1="0" x2={Math.cos(a) * 0.45} y2={-Math.sin(a) * 0.45} class="dir" />
        <circle
          cx={Math.cos(a) * 0.45}
          cy={-Math.sin(a) * 0.45}
          r="0.08"
          class="turn"
          role="slider"
          tabindex="-1"
          aria-valuenow={t.yaw}
          onpointerdown={(e) => { e.stopPropagation(); selection.select({ kind: "tag", id: String(t.id) }); drag = { kind: "turn", id: t.id }; svg?.setPointerCapture(e.pointerId); }}
        />
        <rect
          x="-0.17"
          y="-0.17"
          width="0.34"
          height="0.34"
          rx="0.04"
          transform="rotate({-t.yaw})"
          class="body"
          role="button"
          tabindex="-1"
          onpointerdown={(e) => { e.stopPropagation(); selection.select({ kind: "tag", id: String(t.id) }); drag = { kind: "tag", id: t.id }; svg?.setPointerCapture(e.pointerId); }}
        />
        <text y="0.06" class="id">{t.id}</text>
      </g>
    {/each}
    <g class="robot" transform="translate({field.robotPose.x},{W - field.robotPose.y}) rotate({-field.robotPose.heading})">
      <rect x="-0.45" y="-0.45" width="0.9" height="0.9" rx="0.08" class="rb" role="button" tabindex="-1" onpointerdown={(e) => { e.stopPropagation(); drag = { kind: "robot" }; svg?.setPointerCapture(e.pointerId); }} />
      <path d="M0.15,-0.2 L0.4,0 L0.15,0.2" class="fwd" />
      <circle cx="0.75" cy="0" r="0.09" class="turn" role="slider" tabindex="-1" aria-valuenow={field.robotPose.heading} onpointerdown={(e) => { e.stopPropagation(); drag = { kind: "robot-turn" }; svg?.setPointerCapture(e.pointerId); }} />
    </g>
  </svg>
  <div class="seen-list">
    <b>{visible.size}</b> tags in view from here
    {#each cluster.cameras.filter((c) => robot.placements[c.resourceId]) as c (c.resourceId)}<i style:background={colorVar(identity.get(c.resourceId).color)} data-tip={identity.name(c.resourceId, c.name)}></i>{/each}
  </div>
</div>

<style>
  .count {
    font-size: 11.5px;
    color: var(--fg-3);
    white-space: nowrap;
  }
  .wrap {
    position: relative;
    flex: 1;
    min-height: 0;
    display: flex;
    padding: 6px;
  }
  svg {
    flex: 1;
    min-height: 0;
    width: 100%;
    user-select: none;
  }
  .floor {
    fill: color-mix(in oklab, var(--s2) 70%, var(--ok) 4%);
  }
  .gl {
    stroke: var(--line);
    stroke-width: 0.012;
  }
  .mid {
    stroke: var(--line-strong);
    stroke-width: 0.03;
    stroke-dasharray: 0.15 0.1;
  }
  .wall {
    fill: none;
    stroke: var(--fg-3);
    stroke-width: 0.05;
  }
  .fov {
    opacity: 0.12;
    pointer-events: none;
  }
  .tag .body {
    fill: var(--s1);
    stroke: var(--fg-3);
    stroke-width: 0.03;
    cursor: move;
  }
  .tag.seen .body {
    stroke: #22d3ee;
    fill: color-mix(in oklab, #22d3ee 20%, var(--s1));
  }
  .tag.sel .body {
    stroke: var(--accent);
    stroke-width: 0.05;
  }
  .dir {
    stroke: var(--fg-3);
    stroke-width: 0.025;
  }
  .tag.sel .dir {
    stroke: var(--accent);
  }
  .turn {
    fill: var(--s1);
    stroke: var(--fg-3);
    stroke-width: 0.025;
    cursor: grab;
    opacity: 0;
  }
  .tag:hover .turn,
  .tag.sel .turn,
  .robot:hover .turn {
    opacity: 1;
  }
  .id {
    font-size: 0.18px;
    font-weight: 700;
    font-family: var(--font-code);
    text-anchor: middle;
    fill: var(--fg);
    pointer-events: none;
  }
  .rb {
    fill: color-mix(in oklab, var(--accent) 20%, var(--s1));
    stroke: var(--accent);
    stroke-width: 0.04;
    cursor: move;
  }
  .fwd {
    fill: none;
    stroke: var(--accent);
    stroke-width: 0.06;
    stroke-linecap: round;
    stroke-linejoin: round;
    pointer-events: none;
  }
  .seen-list {
    position: absolute;
    left: 14px;
    bottom: 12px;
    display: flex;
    align-items: center;
    gap: 5px;
    padding: 3px 9px;
    font-size: 11.5px;
    color: var(--fg-2);
    background: color-mix(in oklab, var(--s1) 85%, transparent);
    border: 1px solid var(--line);
    border-radius: var(--r-1);
  }
  .seen-list b {
    font-family: var(--font-code);
    color: var(--fg);
  }
  .seen-list i {
    width: 8px;
    height: 8px;
    border-radius: 50%;
  }
</style>
