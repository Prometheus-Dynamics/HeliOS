<script lang="ts">
  // The 16-LED ring as the device shows it. `leds` events on the event stream set what is drawn:
  // device frames exactly, a named look animated here (the same maths as the ring, in
  // lemnos-light's terms). Changes ease between events. Reduced motion draws one still frame.
  import { api } from "#lib/api/client.js";
  import type { LedEvent } from "#lib/api/leds.js";
  import { hexOf, type Rgb } from "#lib/leds/colour.js";
  import { blackFrame } from "#lib/leds/render.js";
  import { angleOf, approach, LEDS, mod } from "#lib/leds/ring.js";
  import { frameFor, type LedView } from "#lib/leds/states.js";
  import { reducedMotion } from "#lib/ui/motion.js";

  let { initial, label = "" }: { initial: LedView | null; label?: string } = $props();

  /** How fast a change eases in (ms): about 63% of the way in one time constant. */
  const TAU_MS = 110;
  const RADIUS = 72;
  /** Spots and the sheen are drawn as glowing dots, eased by position and level. */
  interface Dot {
    at: number;
    level: number;
  }

  let seq = 0;
  const uid = `led-ring-${++seq}`;

  let event = $state<LedEvent | null>(null);
  let connected = $state(false);
  const view = $derived<LedView | null>(event ? { state: event.state, look: event.look, brightness: event.brightness, leds: event.leds } : initial);

  let colours = $state<string[]>(Array.from({ length: LEDS }, () => "#000000"));
  let spots = $state<Dot[]>([]);
  let sheen = $state<Dot[]>([]);

  function point(position: number): { x: number; y: number } {
    const angle = angleOf(position);
    return { x: RADIUS * Math.sin(angle), y: -RADIUS * Math.cos(angle) };
  }
  const ledPoints = Array.from({ length: LEDS }, (_, i) => point(i));

  // Subscribe to the leds kind only while the ring is on screen.
  $effect(() =>
    api.events(
      {
        onOpen: () => (connected = true),
        onError: () => (connected = false),
        onEvent: (e) => {
          if (e.type === "leds") event = e.data as LedEvent;
        },
      },
      ["leds"],
    ),
  );

  /** Ease a list of dots toward its targets; a dot with no target fades out where it is. */
  function stepDots(current: Dot[], target: Dot[], dt: number): Dot[] {
    const next: Dot[] = [];
    const count = Math.max(current.length, target.length);
    for (let k = 0; k < count; k++) {
      const want = target[k] ?? { at: current[k]?.at ?? 0, level: 0 };
      const now = current[k] ?? { at: want.at, level: 0 };
      // Go the short way round the ring.
      const delta = mod(want.at - now.at + LEDS / 2, LEDS) - LEDS / 2;
      const moved: Dot = {
        at: mod(approach(now.at, now.at + delta, dt, TAU_MS), LEDS),
        level: approach(now.level, want.level, dt, TAU_MS),
      };
      if (k < target.length || moved.level > 0.01) next.push(moved);
    }
    return next;
  }

  $effect(() => {
    if (reducedMotion) {
      // No motion: draw the current frame once, and again when it changes.
      const v = view;
      colours = (v ? frameFor(v, 0) : blackFrame()).map(hexOf);
      spots = (event?.spots ?? []).map((s) => ({ at: s.at, level: s.level }));
      sheen = event?.sheen == null ? [] : [{ at: event.sheen, level: 1 }];
      return;
    }
    let raf = 0;
    let last = 0;
    let lookKey = "";
    let lookStart = 0;
    let current: Rgb[] | null = null;
    const tick = (now: number) => {
      const dt = last ? now - last : 16;
      last = now;
      // The look's clock restarts when the state or look changes.
      const v = view;
      const key = `${v?.state}|${v?.look}`;
      if (key !== lookKey) {
        lookKey = key;
        lookStart = now;
      }
      const target = v ? frameFor(v, now - lookStart) : blackFrame();
      if (!current) current = target.map((c) => [...c] as Rgb);
      for (let i = 0; i < LEDS; i++) {
        for (let ch = 0; ch < 3; ch++) current[i][ch] = approach(current[i][ch], target[i][ch], dt, TAU_MS);
      }
      colours = current.map(hexOf);
      spots = stepDots(spots, (event?.spots ?? []).map((s) => ({ at: s.at, level: s.level })), dt);
      sheen = stepDots(sheen, event?.sheen == null ? [] : [{ at: event.sheen, level: 1 }], dt);
      raf = requestAnimationFrame(tick);
    };
    raf = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(raf);
  });
</script>

<figure class="wrap">
  <svg class="led-ring" viewBox="-100 -100 200 200" role="img" aria-label="Preview of the 16 LEDs, LED 0 at the top, clockwise">
    <defs>
      <filter id="{uid}-glow" x="-60%" y="-60%" width="220%" height="220%">
        <feGaussianBlur stdDeviation="5" />
      </filter>
      <filter id="{uid}-soft" x="-100%" y="-100%" width="300%" height="300%">
        <feGaussianBlur stdDeviation="3" />
      </filter>
    </defs>
    <circle class="plate" r="94" />
    <g filter="url(#{uid}-glow)" opacity="0.85">
      {#each colours as c, i (i)}
        <circle cx={ledPoints[i].x} cy={ledPoints[i].y} r="11" fill={c} />
      {/each}
    </g>
    <g>
      {#each colours as c, i (i)}
        <circle class="led" cx={ledPoints[i].x} cy={ledPoints[i].y} r="6.5" fill={c} />
      {/each}
    </g>
    <g filter="url(#{uid}-soft)">
      {#each spots as s, k (k)}
        {@const p = point(s.at)}
        <circle cx={p.x} cy={p.y} r={5 + 6 * s.level} fill="#28c8ff" opacity={s.level} />
      {/each}
      {#each sheen as s, k (k)}
        {@const p = point(s.at)}
        <circle cx={p.x} cy={p.y} r={5 + 4 * s.level} fill="#ffffff" opacity={0.55 * s.level} />
      {/each}
    </g>
    <text class="zero" x="0" y="-84">0</text>
  </svg>
  <figcaption>
    <span class="dot" class:live={connected && event !== null}></span>
    <span class="text">{event || initial ? label || "Live" : "Waiting for the device"}</span>
  </figcaption>
</figure>

<style>
  .wrap {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 6px;
    margin: 0;
    padding: 12px 10px 6px;
  }
  .led-ring {
    width: min(210px, 100%);
    height: auto;
    overflow: visible;
  }
  .plate {
    fill: var(--inset);
    stroke: var(--line);
    stroke-width: 1;
  }
  .zero {
    fill: var(--fg-3);
    font-family: var(--font-code);
    font-size: 9px;
    text-anchor: middle;
  }
  .led {
    stroke: rgba(255, 255, 255, 0.05);
    stroke-width: 0.5;
  }
  figcaption {
    display: flex;
    align-items: center;
    gap: 6px;
    max-width: 100%;
    font-size: 11.5px;
    color: var(--fg-2);
  }
  .text {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .dot {
    width: 6px;
    height: 6px;
    flex-shrink: 0;
    border-radius: 50%;
    background: var(--fg-4);
  }
  .dot.live {
    background: var(--ok);
    box-shadow: 0 0 6px var(--ok);
  }
</style>
