<script lang="ts">
  // Guided calibration: pick the camera and board, capture views until the
  // coverage map is full, solve, check the error per view, save. Every step
  // shows what "good" looks like so nobody needs to know the maths.
  import Icon from "#lib/components/common/Icon.svelte";
  import { identity } from "#lib/core/identity.svelte.js";
  import { selection } from "#lib/core/selection.svelte.js";
  import { pane as makePane, workspaces } from "#lib/core/workspace.svelte.js";
  import { downloadJson } from "#lib/files.js";
  import Badge from "#lib/kit/Badge.svelte";
  import IconButton from "#lib/kit/IconButton.svelte";
  import Picker from "#lib/kit/Picker.svelte";
  import Prop from "#lib/kit/Prop.svelte";
  import Switch from "#lib/kit/Switch.svelte";
  import { calibration as cal, COVER_COLS, COVER_ROWS } from "#lib/stores/calibration.svelte.js";
  import { cluster } from "#lib/stores/cluster.svelte.js";
  import Feed from "#lib/vision/Feed.svelte";
  import { DEFAULT_OVERLAYS } from "#lib/vision/overlays.js";
  import PaneBar from "#lib/workspace/PaneBar.svelte";
  import type { PaneProps } from "#lib/workspace/panes.js";
  import Empty from "./Empty.svelte";
  import { follow } from "./follow.svelte";

  let { pane, ws }: PaneProps = $props();
  const target = follow(() => pane, () => ws, "camera", () => cluster.cameras[0]?.resourceId);
  const camera = $derived(target.id ? cluster.camera(target.id) : undefined);
  const existing = $derived(camera ? cal.saved[cal.key(camera)] : undefined);
  const cover = $derived(cal.coverage());
  const inView = $derived(camera ? cal.boardIn(camera) : null);
  const ready = $derived(cal.captures.length >= 12 && cal.coveredFraction >= 0.6);
  const STEPS = [
    { id: "setup", label: "Board" },
    { id: "capture", label: "Capture" },
    { id: "solve", label: "Solve" },
    { id: "done", label: "Check & save" },
  ] as const;
  const stepIndex = $derived(STEPS.findIndex((s) => s.id === cal.step));

  $effect(() => {
    void cluster.frame;
    if (camera) cal.autoTick(camera);
  });

  const quality = $derived(cal.result ? (cal.result.rms < 0.3 ? { tone: "ok", text: "Excellent" } : cal.result.rms < 0.6 ? { tone: "warn", text: "Usable" } : { tone: "err", text: "Redo it" }) : null);
  const boardLabel = $derived(`${cal.board.kind === "charuco" ? "ChArUco" : cal.board.kind === "aprilgrid" ? "AprilGrid" : "Chessboard"} ${cal.board.cols}×${cal.board.rows}, ${cal.board.square} mm`);
</script>

<PaneBar>
  <Picker
    label="Camera"
    icon="camera"
    value={camera?.resourceId}
    options={cluster.cameras.map((c) => ({ id: c.resourceId, name: identity.name(c.resourceId, c.name) }))}
    onpick={(id) => (target.pinned ? target.pin(id) : selection.select({ kind: "camera", id }))}
    pinned={target.pinned}
    ontogglepin={() => target.toggle()}
  />
  <span class="sep"></span>
  <div class="steps">
    {#each STEPS as s, i (s.id)}
      <span class="step" class:on={i === stepIndex} class:done={i < stepIndex}>
        <i>{#if i < stepIndex}<Icon name="check" size={10} stroke={3} />{:else}{i + 1}{/if}</i>{s.label}
      </span>
    {/each}
  </div>
</PaneBar>

{#if camera}
  <div class="cal">
    <div class="view">
      <Feed {camera} overlays={{ ...DEFAULT_OVERLAYS, ids: false, histogram: false, hud: true }} />
      {#if cal.step === "capture" || cal.step === "done"}
        <div class="cover" aria-label="Coverage map">
          {#each cover as n, i (i)}
            <span class:hit={n > 0} class:busy={!!inView && inView.some(([x, y]) => Math.floor(x * COVER_COLS) === i % COVER_COLS && Math.floor(y * COVER_ROWS) === Math.floor(i / COVER_COLS))} style:--k={Math.min(1, n / 12)}></span>
          {/each}
        </div>
      {/if}
    </div>

    <aside class="side">
      {#if cal.step === "setup"}
        <h3>Calibrate {identity.name(camera.resourceId, camera.name)}</h3>
        <p class="lead">Calibration measures the lens so tag distances and angles come out right. It takes about two minutes.</p>
        {#if existing}
          <div class="have"><Icon name="circle-check" size={14} /> Calibrated at {existing.width}×{existing.height}, {existing.rms.toFixed(2)} px RMS, {existing.views} views.</div>
        {/if}
        <Prop label="Resolution"><span class="mono">{camera.settings.width}×{camera.settings.height}</span></Prop>
        <Prop label="Board"><span class="small">{boardLabel}</span></Prop>
        <div class="tip"><Icon name="printer" size={13} /> No board yet? <button type="button" onclick={() => workspaces.open(ws, makePane("board"))}>Make one in Board maker</button>, print it at 100% and tape it to something flat.</div>
        <button type="button" class="primary" onclick={() => cal.restart()}>Start capturing <Icon name="arrow-right" size={13} /></button>
      {:else if cal.step === "capture"}
        <h3>Capture views</h3>
        <p class="lead">Move the board around the whole image, close and far, and tilt it. Fill the grid; green cells have enough corners.</p>
        <div class="meters">
          <div><span>Views</span><b>{cal.captures.length}</b><i>/ 12+</i></div>
          <div><span>Coverage</span><b>{Math.round(cal.coveredFraction * 100)}%</b><i>/ 60%+</i></div>
          <div><span>Angles</span><b>{cal.tiltVariety}</b><i>/ 3+</i></div>
        </div>
        <Prop label="Auto-capture" help="Takes a view whenever the board is in sight and has moved."><Switch checked={cal.auto} onchange={(v) => (cal.auto = v)} /></Prop>
        <div class="row">
          <button type="button" class="secondary" disabled={!inView} onclick={() => cal.capture(camera)}><Icon name="player-record" size={13} /> Capture{inView ? "" : " (no board)"}</button>
          <button type="button" class="primary" disabled={!ready} onclick={() => cal.solve(camera)}>Solve <Icon name="arrow-right" size={13} /></button>
        </div>
        {#if !ready}<p class="small dim">Solve unlocks at 12 views and 60% coverage.</p>{/if}
      {:else if cal.step === "solve"}
        <h3>Solving…</h3>
        <p class="lead">Fitting the lens model to {cal.captures.length} views.</p>
        <div class="spin"><Icon name="loader-2" size={22} class="animate-spin" /></div>
      {:else if cal.result}
        {@const r = cal.result}
        <h3>Result <Badge tone={quality?.tone as "ok"} text={quality?.text ?? ""} /></h3>
        <div class="meters">
          <div><span>RMS error</span><b>{r.rms.toFixed(3)}</b><i>px</i></div>
          <div><span>Views</span><b>{r.views}</b></div>
        </div>
        <div class="errs" aria-label="Error per view">
          {#each cal.captures as c, i (i)}<span style:height="{Math.min(100, ((c.error ?? 0) / 1) * 100)}%" class:bad={(c.error ?? 0) > r.rms * 1.4} data-tip="view {i + 1}: {c.error} px"></span>{/each}
        </div>
        <Prop label="fx · fy"><span class="mono">{r.fx} · {r.fy}</span></Prop>
        <Prop label="cx · cy"><span class="mono">{r.cx} · {r.cy}</span></Prop>
        <Prop label="Distortion" stacked><span class="mono small">{r.dist.join("  ")}</span></Prop>
        <div class="row">
          <button type="button" class="secondary" onclick={() => cal.rejectOutliers(camera)}>Drop worst & re-solve</button>
          <button type="button" class="primary" onclick={() => { cal.apply(camera); cal.step = "setup"; }}><Icon name="device-floppy" size={13} /> Save</button>
        </div>
        <div class="row">
          <button type="button" class="link" onclick={() => cal.restart()}>Start over</button>
          <button type="button" class="link" onclick={() => downloadJson(`${camera.resourceId}-${r.width}x${r.height}.calibration.json`, { format: "helios.calibration", schema_version: 1, ...r, camera_matrix: [[r.fx, 0, r.cx], [0, r.fy, r.cy], [0, 0, 1]], dist_coeffs: r.dist })}>Export JSON</button>
        </div>
      {/if}
    </aside>
  </div>
{:else}
  <Empty icon="camera" text="Pick a camera to calibrate." />
{/if}

<style>
  .steps {
    display: flex;
    gap: 2px;
  }
  .step {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    padding: 0 8px 0 4px;
    height: 22px;
    font-size: 11.5px;
    color: var(--fg-3);
    border-radius: var(--r-1);
  }
  .step i {
    display: grid;
    place-items: center;
    width: 16px;
    height: 16px;
    font-style: normal;
    font-size: 10px;
    font-weight: 700;
    border-radius: 50%;
    border: 1px solid var(--line-strong);
  }
  .step.on {
    color: var(--fg);
    background: var(--accent-tint);
  }
  .step.on i {
    background: var(--accent);
    border-color: var(--accent);
    color: var(--on-accent);
  }
  .step.done i {
    background: var(--ok);
    border-color: var(--ok);
    color: var(--s0);
  }
  .cal {
    flex: 1;
    display: flex;
    min-height: 0;
  }
  .view {
    position: relative;
    flex: 1;
    display: flex;
    min-width: 0;
  }
  .cover {
    position: absolute;
    inset: 0;
    display: grid;
    grid-template-columns: repeat(8, 1fr);
    grid-template-rows: repeat(5, 1fr);
    pointer-events: none;
  }
  .cover span {
    border: 1px solid rgba(255, 255, 255, 0.08);
  }
  .cover span.hit {
    background: color-mix(in oklab, var(--ok) calc(var(--k) * 32%), transparent);
  }
  .cover span.busy {
    box-shadow: inset 0 0 0 2px color-mix(in oklab, var(--warn) 70%, transparent);
  }
  .side {
    width: 290px;
    flex-shrink: 0;
    overflow-y: auto;
    border-left: 1px solid var(--line);
    padding-bottom: 10px;
  }
  h3 {
    display: flex;
    align-items: center;
    gap: 8px;
    margin: 0;
    padding: 10px 10px 4px;
    font-size: 14px;
  }
  .lead {
    margin: 0;
    padding: 0 10px 8px;
    font-size: 12px;
    color: var(--fg-2);
    line-height: 1.45;
  }
  .have {
    display: flex;
    gap: 6px;
    margin: 0 10px 8px;
    padding: 6px 8px;
    font-size: 11.5px;
    color: var(--ok);
    background: var(--ok-bg);
    border-radius: var(--r-2);
  }
  .tip {
    margin: 8px 10px;
    font-size: 11.5px;
    color: var(--fg-3);
    line-height: 1.5;
  }
  .tip button,
  .link {
    color: var(--accent-fg);
    font-size: 11.5px;
  }
  .meters {
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    gap: 1px;
    margin: 0 10px 8px;
    background: var(--line);
    border: 1px solid var(--line);
    border-radius: var(--r-2);
    overflow: hidden;
  }
  .meters div {
    display: flex;
    flex-direction: column;
    padding: 6px 8px;
    background: var(--s1);
  }
  .meters span {
    font-size: 10px;
    color: var(--fg-3);
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }
  .meters b {
    font-family: var(--font-code);
    font-size: 16px;
  }
  .meters i {
    font-style: normal;
    font-size: 10.5px;
    color: var(--fg-3);
  }
  .row {
    display: flex;
    gap: 6px;
    padding: 8px 10px 0;
    justify-content: space-between;
  }
  .primary,
  .secondary {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 6px;
    height: 28px;
    padding: 0 12px;
    font-size: 12px;
    font-weight: 600;
    border-radius: var(--r-2);
    flex: 1;
  }
  .primary {
    color: var(--on-accent);
    background: var(--accent);
  }
  .side > .primary {
    margin: 8px 10px;
    width: calc(100% - 20px);
  }
  .secondary {
    color: var(--fg);
    border: 1px solid var(--line-strong);
  }
  .primary:disabled,
  .secondary:disabled {
    opacity: 0.4;
  }
  .errs {
    display: flex;
    align-items: flex-end;
    gap: 2px;
    height: 46px;
    margin: 0 10px 8px;
    padding: 4px;
    background: var(--inset);
    border-radius: var(--r-1);
  }
  .errs span {
    flex: 1;
    background: var(--ok);
    border-radius: 1px;
    min-height: 2px;
  }
  .errs span.bad {
    background: var(--err);
  }
  .spin {
    display: grid;
    place-items: center;
    padding: 20px;
    color: var(--accent);
  }
  .mono {
    font-family: var(--font-code);
    font-size: 11.5px;
  }
  .small {
    font-size: 11.5px;
  }
  .dim {
    padding: 4px 10px;
    color: var(--fg-3);
  }
</style>
