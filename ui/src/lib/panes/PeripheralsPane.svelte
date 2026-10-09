<script lang="ts">
  // A device's hardware: fan and curve, LEDs, GPIO pins, IMU and power rails.
  import { identity } from "#lib/core/identity.svelte.js";
  import { selection } from "#lib/core/selection.svelte.js";
  import Badge from "#lib/kit/Badge.svelte";
  import CurveEditor from "#lib/kit/CurveEditor.svelte";
  import Picker from "#lib/kit/Picker.svelte";
  import Prop from "#lib/kit/Prop.svelte";
  import Choice from "#lib/kit/Choice.svelte";
  import Num from "#lib/kit/Num.svelte";
  import Section from "#lib/kit/Section.svelte";
  import Seg from "#lib/kit/Seg.svelte";
  import Slider from "#lib/kit/Slider.svelte";
  import Switch from "#lib/kit/Switch.svelte";
  import { cluster } from "#lib/stores/cluster.svelte.js";
  import { system } from "#lib/stores/system.svelte.js";
  import PaneBar from "#lib/workspace/PaneBar.svelte";
  import type { PaneProps } from "#lib/workspace/panes.js";
  import Empty from "./Empty.svelte";
  import { follow } from "./follow.svelte";

  let { pane, ws }: PaneProps = $props();
  const target = follow(() => pane, () => ws, "device", () => cluster.nodes[0]?.id);
  const node = $derived(target.id ? cluster.node(target.id) : undefined);
  const hw = $derived(node ? system.of(node.id) : undefined);
  const empty = $derived(hw && !hw.fan && !hw.leds.length && !hw.gpio.length && !hw.imu && !hw.power.length);
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
</PaneBar>

{#if node && hw && !empty}
  <div class="scroll">
    {#if hw.fan}
      {@const fan = hw.fan}
      <Section title="Fan" key="hw-fan">
        {#snippet actions()}<span class="live"><b>{fan.rpm}</b> rpm · <b>{Math.round(fan.duty * 100)}</b>%</span>{/snippet}
        <Prop label="Control">
          <Seg label="Fan control" value={fan.mode} options={[{ value: "auto", label: "Curve" }, { value: "manual", label: "Fixed" }]} onchange={(v) => (fan.mode = v)} />
        </Prop>
        {#if fan.mode === "manual"}
          <Prop label="Duty"><Slider value={Math.round(fan.duty * 100)} min={0} max={100} unit="%" onchange={(v) => (fan.duty = v / 100)} /></Prop>
        {/if}
        <div class="curve">
          <CurveEditor points={fan.curve} xmin={30} xmax={85} xunit="°" marker={{ x: node.tempC, y: fan.duty }} disabled={fan.mode === "manual"} onchange={(p) => (fan.curve = p)} />
          <p class="hint">Drag points · double-click to add or remove · the dashed line is the current temperature ({node.tempC.toFixed(0)} °C).</p>
        </div>
      </Section>
    {/if}

    {#if hw.leds.length}
      <Section title="LEDs" key="hw-leds" count={hw.leds.length}>
        {#each hw.leds as led (led.id)}
          <div class="led">
            <div class="led-head">
              <input type="color" value={led.color} aria-label="{led.name} colour" oninput={(e) => (led.color = (e.currentTarget as HTMLInputElement).value)} />
              <b>{led.name}</b>
              <span class="dim">{led.count} px</span>
            </div>
            <div class="strip" style:--led={led.color} style:opacity={0.25 + led.brightness * 0.75}>
              {#each Array.from({ length: Math.min(led.count, 30) }) as _, i (i)}<i class="p-{led.pattern}" style:animation-delay="{i * 40}ms"></i>{/each}
            </div>
            <div class="led-ctl">
              <Seg label="Pattern" value={led.pattern} options={[{ value: "solid", label: "Solid" }, { value: "blink", label: "Blink" }, { value: "breathe", label: "Breathe" }, { value: "status", label: "Status" }]} onchange={(v) => (led.pattern = v)} />
              <Slider value={Math.round(led.brightness * 100)} min={0} max={100} unit="%" label="Brightness" onchange={(v) => (led.brightness = v / 100)} />
            </div>
          </div>
        {/each}
      </Section>
    {/if}

    {#if hw.gpio.length}
      <Section title="GPIO" key="hw-gpio" count={hw.gpio.length}>
        <div class="gpio">
          {#each hw.gpio as g (g.pin)}
            <div class="pin">
              <span class="num">{g.pin}</span>
              <span class="pname">{g.name}</span>
              <span class="owner">{g.owner ?? "free"}</span>
              <span class="mode">{g.mode}</span>
              <span class="val">
                {#if g.mode === "out"}
                  <Switch checked={g.value > 0} disabled={Boolean(g.owner)} label="{g.name} level" onchange={(v) => (g.value = v ? 1 : 0)} />
                {:else if g.mode === "pwm"}
                  <Slider value={Math.round(g.value * 100)} min={0} max={100} unit="%" disabled={Boolean(g.owner)} onchange={(v) => (g.value = v / 100)} />
                {:else}
                  <span class="lvl" class:hi={g.value > 0}>{g.value > 0 ? "HIGH" : "LOW"}</span>
                {/if}
              </span>
            </div>
          {/each}
        </div>
        <p class="hint">Pins owned by a service are read-only here; release them from the owner first.</p>
      </Section>
    {/if}

    {#if hw.imu}
      {@const imu = hw.imu}
      <Section title="IMU" key="hw-imu">
        <div class="imu">
          <svg viewBox="-50 -50 100 100" class="compass" aria-label="Heading {imu.yaw.toFixed(0)}°">
            <circle r="44" class="ring" />
            {#each Array.from({ length: 12 }) as _, i (i)}<line y1="-44" y2={i % 3 ? "-40" : "-36"} transform="rotate({i * 30})" class="tick" />{/each}
            <g transform="rotate({imu.yaw})"><path d="M0,-34 L6,4 L0,0 L-6,4Z" class="needle" /></g>
            <text y="18" class="hd">{imu.yaw.toFixed(1)}°</text>
          </svg>
          <div class="att">
            <Prop label="Yaw · pitch · roll"><span class="mono">{imu.yaw.toFixed(2)}° · {imu.pitch.toFixed(2)}° · {imu.roll.toFixed(2)}°</span></Prop>
            <Prop label="Temperature"><span class="mono">{imu.temperatureC.toFixed(1)} °C</span></Prop>
            <Prop label="Gyro bias"><span class="mono">{imu.gyroBias.map((b) => b.toFixed(4)).join(" ")} °/s</span></Prop>
            <Prop label="Actions">
              <button type="button" class="btn" onclick={() => (imu.yaw = 0)}>Zero yaw</button>
              <button type="button" class="btn" onclick={() => system.calibrateGyro(node.id)}>Calibrate gyro</button>
            </Prop>
          </div>
        </div>
        <Prop label="Output rate"><Choice value={imu.rateHz} options={[100, 200, 400, 1000, 2000].map((v) => ({ value: v, label: `${v} Hz` }))} onchange={(v) => (imu.rateHz = v)} /></Prop>
        <Prop label="Gyro range"><Choice value={imu.gyroRange} options={[125, 250, 500, 1000, 2000].map((v) => ({ value: v, label: `±${v} °/s` }))} onchange={(v) => (imu.gyroRange = v)} /></Prop>
        <Prop label="Accel range"><Choice value={imu.accelRange} options={[3, 6, 12, 24].map((v) => ({ value: v, label: `±${v} g` }))} onchange={(v) => (imu.accelRange = v)} /></Prop>
        <Prop label="Low-pass filter"><Choice value={imu.lpfHz} options={[23, 47, 116, 230, 532].map((v) => ({ value: v, label: `${v} Hz` }))} onchange={(v) => (imu.lpfHz = v)} /></Prop>
        <Prop label="Mount (r, p, y °)" help="How the IMU board is rotated on the robot.">
          {#each [0, 1, 2] as i (i)}
            <Num value={imu.mount[i]} min={-180} max={180} step={0.5} width={60} label={["Roll", "Pitch", "Yaw"][i]} onchange={(v) => (imu.mount[i] = v)} />
          {/each}
        </Prop>
      </Section>
    {/if}

    {#if hw.power.length}
      <Section title="Power" key="hw-power" count={hw.power.length}>
        {#each hw.power as r (r.rail)}
          <Prop label={r.rail}>
            <span class="mono">{r.volts.toFixed(2)} V</span>
            <span class="mono dim">{r.amps.toFixed(2)} A</span>
            <span class="mono">{(r.volts * r.amps).toFixed(2)} W</span>
          </Prop>
        {/each}
        {#if hw.throttled}<Badge tone="warn" text="Throttled: undervoltage seen" />{/if}
      </Section>
    {/if}
  </div>
{:else if node}
  <Empty icon="plug" text="{identity.name(node.id, node.name)} reports no controllable hardware." />
{/if}

<style>
  .scroll {
    flex: 1;
    overflow-y: auto;
  }
  .live {
    font-size: 11px;
    color: var(--fg-3);
  }
  .live b {
    font-family: var(--font-code);
    color: var(--fg);
  }
  .curve {
    padding: 4px 14px 2px;
  }
  .hint {
    margin: 4px 10px 2px;
    font-size: 10.5px;
    color: var(--fg-3);
  }
  .led {
    padding: 6px 10px;
    border-top: 1px solid color-mix(in oklab, var(--line) 60%, transparent);
  }
  .led:first-child {
    border-top: 0;
  }
  .led-head {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 12px;
  }
  .led-head input[type="color"] {
    width: 22px;
    height: 22px;
    padding: 0;
    border: 1px solid var(--line-strong);
    border-radius: var(--r-1);
    background: none;
    cursor: pointer;
  }
  .led-head b {
    font-weight: 600;
  }
  .dim {
    color: var(--fg-3);
    font-size: 11px;
  }
  .strip {
    display: flex;
    gap: 2px;
    margin: 6px 0;
  }
  .strip i {
    flex: 1;
    height: 6px;
    border-radius: 2px;
    background: var(--led);
    box-shadow: 0 0 6px var(--led);
  }
  .p-blink {
    animation: blink 1s steps(2) infinite;
  }
  .p-breathe {
    animation: breathe 2.4s ease-in-out infinite;
  }
  @keyframes blink {
    50% {
      opacity: 0.15;
    }
  }
  @keyframes breathe {
    50% {
      opacity: 0.25;
    }
  }
  .led-ctl {
    display: flex;
    align-items: center;
    gap: 10px;
  }
  .gpio {
    display: flex;
    flex-direction: column;
  }
  .pin {
    display: grid;
    grid-template-columns: 26px 1fr 80px 34px 120px;
    align-items: center;
    gap: 8px;
    height: 26px;
    padding: 0 10px;
    font-size: 12px;
  }
  .pin:hover {
    background: color-mix(in oklab, var(--s3) 50%, transparent);
  }
  .num {
    font-family: var(--font-code);
    font-size: 11px;
    color: var(--fg-3);
  }
  .pname {
    font-family: var(--font-code);
    font-size: 11.5px;
  }
  .owner {
    font-size: 11px;
    color: var(--fg-3);
  }
  .mode {
    font-size: 10.5px;
    text-transform: uppercase;
    color: var(--info);
  }
  .val {
    display: flex;
    justify-content: flex-end;
  }
  .lvl {
    font-family: var(--font-code);
    font-size: 10.5px;
    color: var(--fg-3);
  }
  .lvl.hi {
    color: var(--ok);
  }
  .imu {
    display: flex;
    gap: 6px;
    align-items: center;
    padding-left: 10px;
  }
  .compass {
    width: 96px;
    height: 96px;
    flex-shrink: 0;
  }
  .ring {
    fill: var(--inset);
    stroke: var(--line-strong);
  }
  .tick {
    stroke: var(--fg-3);
  }
  .needle {
    fill: var(--accent);
  }
  .hd {
    font-size: 10px;
    font-family: var(--font-code);
    fill: var(--fg-2);
    text-anchor: middle;
  }
  .att {
    flex: 1;
  }
  .mono {
    font-family: var(--font-code);
    font-size: 11.5px;
  }
  .btn + .btn {
    margin-left: 4px;
  }
  .btn {
    height: 22px;
    padding: 0 8px;
    font-size: 11.5px;
    border: 1px solid var(--line-strong);
    border-radius: var(--r-1);
  }
  .btn:hover {
    background: var(--s3);
  }
</style>
