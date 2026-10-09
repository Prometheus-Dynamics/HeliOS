<script lang="ts">
  // Where every camera sits on the robot, as numbers you can type or scrub,
  // plus the CAD model's units and placement. Exports robot-to-camera
  // transforms for robot code.
  import Icon from "#lib/components/common/Icon.svelte";
  import { colorVar, identity } from "#lib/core/identity.svelte.js";
  import { selection } from "#lib/core/selection.svelte.js";
  import { downloadJson, pickFile } from "#lib/files.js";
  import Choice from "#lib/kit/Choice.svelte";
  import IconButton from "#lib/kit/IconButton.svelte";
  import Num from "#lib/kit/Num.svelte";
  import Prop from "#lib/kit/Prop.svelte";
  import Section from "#lib/kit/Section.svelte";
  import Seg from "#lib/kit/Seg.svelte";
  import { cluster } from "#lib/stores/cluster.svelte.js";
  import { robot, UNITS } from "#lib/stores/robot.svelte.js";
  import PaneBar from "#lib/workspace/PaneBar.svelte";

  const cm = (m: number) => Math.round(m * 1000) / 10;
  const MODEL_TYPES = ".glb,.gltf,.stl,.obj,.ply,.3mf,.step,.stp";
  async function load() {
    const f = await pickFile(MODEL_TYPES);
    if (f) robot.loadModel(f);
  }
</script>

<PaneBar>
  <IconButton icon="upload" label="Load robot CAD" size={24} onclick={load} />
  <IconButton icon="file-export" label="Export robot-to-camera transforms" size={24} onclick={() => downloadJson("robot-cameras.json", robot.exportTransforms())} />
</PaneBar>

<div class="scroll">
  <Section title="Camera mounts" key="mounts-cams" count={Object.keys(robot.placements).length}>
    <div class="mt head"><span>Camera</span><span>x</span><span>y</span><span>z</span><span>roll</span><span>pitch</span><span>yaw</span><span></span></div>
    {#each cluster.cameras as c (c.resourceId)}
      {@const p = robot.placement(c.resourceId)}
      {@const sel = selection.last.camera?.id === c.resourceId}
      <div class="mt" class:sel style:--c={colorVar(identity.get(c.resourceId).color)}>
        <button type="button" class="mh" onclick={() => selection.select({ kind: "camera", id: c.resourceId })} data-tip={p?.part ? `Attached to ${p.part}` : robot.isCustom(c.resourceId) ? "Set by you" : "Guessed from the camera name"}>
          <i></i><b>{identity.name(c.resourceId, c.name)}</b>{#if !robot.isCustom(c.resourceId) && p}<em>guess</em>{/if}
        </button>
        {#if p}
          {#each [0, 1, 2] as i (i)}
            <Num value={cm(p.pos[i])} step={0.5} width={48} label="{['x', 'y', 'z'][i]} cm" onchange={(v) => { const pos = [...p.pos] as [number, number, number]; pos[i] = v / 100; robot.set(c.resourceId, { pos }); }} />
          {/each}
          <Num value={p.roll} min={-180} max={180} step={0.5} width={48} label="Roll °" onchange={(v) => robot.set(c.resourceId, { roll: v })} />
          <Num value={p.pitch} min={-90} max={90} step={0.5} width={48} label="Pitch °" onchange={(v) => robot.set(c.resourceId, { pitch: v })} />
          <Num value={p.yaw} min={-180} max={180} step={0.5} width={48} label="Yaw °" onchange={(v) => robot.set(c.resourceId, { yaw: v })} />
        {:else}
          <button type="button" class="add" onclick={() => robot.set(c.resourceId, { pos: [0, 0, 0.3], yaw: 0, pitch: 15, roll: 0 })}>Add to robot</button>
        {/if}
        <span class="acts">
          <IconButton icon="click" label="Place by clicking the 3D model" size={20} active={robot.picking === c.resourceId} onclick={() => { selection.select({ kind: "camera", id: c.resourceId }); robot.picking = robot.picking === c.resourceId ? null : c.resourceId; }} />
          {#if robot.isCustom(c.resourceId)}<IconButton icon="rotate-clockwise" label="Reset" size={20} onclick={() => robot.reset(c.resourceId)} />{/if}
        </span>
      </div>
    {/each}
    <p class="hint">cm and degrees. Robot frame: x forward, y left, z up from the floor at the chassis centre; pitch up from level, yaw left from forward. Drag a number sideways to scrub, Shift for fine.</p>
  </Section>

  <Section title="Robot model" key="mounts-model">
    {#if robot.info}
      {@const info = robot.info}
      <div class="model">
        <Icon name="cube" size={16} />
        <span class="mn"><b>{info.name}</b><i>{info.parts} parts · {(info.triangles / 1000).toFixed(0)}k triangles</i></span>
        <IconButton icon="trash" label="Remove model" tone="danger" size={22} onclick={() => robot.clearModel()} />
      </div>
      <Prop label="Units" help="Guessed from the model's size; change it if the robot looks tiny or huge.">
        <Choice value={info.unit} options={UNITS} onchange={(v) => (info.unit = v)} />
      </Prop>
      <Prop label="Up axis"><Seg label="Up axis" value={info.up} options={[{ value: "z", label: "Z up" }, { value: "y", label: "Y up" }]} onchange={(v) => (info.up = v)} /></Prop>
      <Prop label="Turn"><Num value={info.yaw} min={-180} max={180} step={90} unit="°" label="Model yaw" onchange={(v) => (info.yaw = v)} /></Prop>
      <Prop label="Offset x · y · z">
        {#each [0, 1, 2] as i (i)}
          <Num value={cm(info.offset[i])} step={0.5} unit="cm" width={66} label="Offset {['x', 'y', 'z'][i]}" onchange={(v) => (info.offset[i] = v / 100)} />
        {/each}
      </Prop>
    {:else}
      <div class="nomodel">
        <span>No model. GLB, glTF, STL, OBJ, PLY, 3MF; export STEP to GLB first.</span>
        <button type="button" class="btn" onclick={load}><Icon name="upload" size={13} /> Load model…</button>
      </div>
    {/if}
  </Section>
</div>

<style>
  .scroll {
    flex: 1;
    overflow-y: auto;
  }
  .mt {
    display: grid;
    grid-template-columns: minmax(70px, 1.4fr) repeat(6, minmax(0, 1fr)) 44px;
    align-items: center;
    gap: 3px;
    padding: 2px 6px 2px 0;
  }
  .mt.head {
    font-size: 10px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    color: var(--fg-3);
    text-align: center;
  }
  .mt.head span:first-child {
    text-align: left;
    padding-left: 10px;
  }
  .mt.sel {
    background: color-mix(in oklab, var(--c) 10%, transparent);
  }
  .mt :global(.num) {
    width: 100% !important;
  }
  .mh {
    display: flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
    height: 26px;
    padding: 0 0 0 10px;
    font-size: 12px;
    text-align: left;
  }
  .mh i {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--c);
    flex-shrink: 0;
  }
  .mh b {
    font-weight: 600;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .mh em {
    font-style: normal;
    font-size: 9.5px;
    color: var(--warn);
  }
  .add {
    grid-column: span 6;
    height: 22px;
    font-size: 11.5px;
    color: var(--fg-2);
    border: 1px dashed var(--line-strong);
    border-radius: var(--r-1);
  }
  .acts {
    display: flex;
    justify-content: flex-end;
  }
  .model {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 4px 10px 6px;
    color: var(--fg-2);
  }
  .mn {
    flex: 1;
    display: flex;
    flex-direction: column;
    min-width: 0;
  }
  .mn b {
    font-size: 12.5px;
    color: var(--fg);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .mn i {
    font-style: normal;
    font-size: 10.5px;
    color: var(--fg-3);
  }
  .nomodel {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 2px 10px 8px;
    font-size: 11.5px;
    color: var(--fg-3);
  }
  .nomodel span {
    flex: 1;
  }
  .btn {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: 24px;
    padding: 0 10px;
    font-size: 12px;
    font-weight: 600;
    color: var(--fg);
    border: 1px solid var(--line-strong);
    border-radius: var(--r-1);
    white-space: nowrap;
  }
  .btn:hover {
    background: var(--s3);
  }
  .hint {
    margin: 6px 10px;
    font-size: 10.5px;
    color: var(--fg-3);
  }
</style>
