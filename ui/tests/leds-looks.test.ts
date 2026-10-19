import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { LOOKS, LOOK_NAMES, type Envelope, type Layer, type Look } from "../src/lib/leds/looks";
import { blend, envelopeAt, renderLayer, renderLook, renderSpec } from "../src/lib/leds/render";
import { approach, ringDistance } from "../src/lib/leds/ring";
import { rgbOf } from "../src/lib/leds/colour";

// The Gaia asset that lemnosd reads. The test reads it, so the TS mirror cannot drift from it.
const TOML = readFileSync(resolve(import.meta.dir, "../../gaia/assets/runtime-config/etc/lemnos/looks.d/helios.toml"), "utf8");

/** Every `[looks."name"]` table in the file, with the text up to the next table. */
function tomlTables(): Map<string, string> {
  const tables = new Map<string, string>();
  const headers = [...TOML.matchAll(/^\[looks\."([^"]+)"\]/gm)];
  headers.forEach((match, i) => {
    const start = match.index! + match[0].length;
    const end = i + 1 < headers.length ? headers[i + 1].index! : TOML.length;
    tables.set(match[1], TOML.slice(start, end));
  });
  return tables;
}

const TABLES = tomlTables();
const approx = (a: number, b: number, eps = 1e-6) => expect(Math.abs(a - b)).toBeLessThanOrEqual(eps);
const lit = (layer: Layer, t: number, progress = 0) => renderLayer(layer, t, progress).map((c) => c[0] + c[1] + c[2]);

describe("look names and layers match the TOML", () => {
  test("every [looks.\"name\"] in the file is in the TS table, and the other way round", () => {
    expect([...TABLES.keys()].sort()).toEqual([...LOOK_NAMES].sort());
  });

  test("each look has the same layers, in the same order, as its TOML table", () => {
    for (const name of LOOK_NAMES) {
      const blocks = [...TABLES.get(name)!.matchAll(/\{ block = "([a-z]+)"/g)].map((m) => m[1]);
      expect({ name, blocks }).toEqual({ name, blocks: LOOKS[name].layers.map((l) => l.block) });
    }
  });
});

describe("comet", () => {
  const comet: Layer = { block: "comet", color: "ffffff", period_ms: 1600, tail: 6, base: 0 };

  test("the head starts on LED 0 and reaches LED 4 a quarter turn later", () => {
    const at0 = lit(comet, 0);
    expect(at0[0]).toBe(3);
    const at400 = lit(comet, 400);
    expect(at400.indexOf(Math.max(...at400))).toBe(4);
  });

  test("the head wraps past the top: LED 0 is half lit when the head is halfway from LED 15", () => {
    const t = (15.5 / 16) * 1600;
    const v = lit(comet, t);
    approx(v[0], 3 * 0.5);
    approx(v[1], 0);
    expect(v[15]).toBeGreaterThan(v[14]);
  });

  test("one full turn later the frame is the same", () => {
    for (const t of [123, 777, 1450]) {
      const a = lit(comet, t);
      const b = lit(comet, t + 1600);
      a.forEach((value, i) => approx(value, b[i], 1e-9));
    }
  });

  test("the tail falls off as (1 - d / tail)^2.2 behind the head, then stops", () => {
    const v = lit(comet, 800); // head on LED 8
    approx(v[8], 3);
    approx(v[7], 3 * (1 - 1 / 6) ** 2.2);
    approx(v[6], 3 * (1 - 2 / 6) ** 2.2);
    expect(v[2]).toBe(0); // six LEDs behind: past the tail
    expect(v[9]).toBe(0); // ahead of the head: dark
  });

  test("a reversed comet runs the other way round", () => {
    const reversed: Layer = { ...comet, reverse: true };
    const v = lit(reversed, 100); // head near LED 15 after a sixteenth of a turn
    expect(v[15]).toBeGreaterThan(v[13]);
    expect(v[0]).toBeGreaterThan(0); // the tail is behind it, on the higher indices
  });

  test("the base keeps every LED at least that bright", () => {
    const glow: Layer = { ...comet, base: 0.18 };
    for (const value of lit(glow, 0)) expect(value).toBeGreaterThanOrEqual(3 * 0.18 - 1e-9);
  });
});

describe("arc", () => {
  const arc: Layer = { block: "arc", fraction: "input", color: "ff0000", track: "000000", head: 0.35 };

  test("fraction input clamps to 0 to 1 progress", () => {
    expect(lit(arc, 0, -1).every((v) => v === 0)).toBe(true);
    expect(lit(arc, 0, 2).every((v) => v === 1 + 0.35 + 0.35 || v === 1)).toBe(true);
  });

  test("the filled part leads clockwise from the top and its last LED is mixed toward white", () => {
    const v = renderLayer(arc, 0, 0.5); // 8 LEDs of the 16
    expect(v[6]).toEqual([1, 0, 0]);
    approx(v[7][0], 1); // the leading LED is red mixed 35% toward white
    approx(v[7][1], 0.35);
    approx(v[7][2], 0.35);
    expect(v[8]).toEqual([0, 0, 0]);
  });

  test("a numeric fraction ignores the progress", () => {
    const fixed: Layer = { ...arc, fraction: 0.25 };
    expect(lit(fixed, 0, 0.9)).toEqual(lit(fixed, 0, 0.1));
  });
});

describe("envelopes", () => {
  test("breathe stays between 1 - depth and 1 and starts at full", () => {
    const env: Envelope = { kind: "breathe", period_ms: 3000, depth: 0.7, easing: "ease-in-out" };
    let lo = Infinity;
    let hi = -Infinity;
    for (let t = 0; t <= 9000; t += 1) {
      const v = envelopeAt(env, t);
      lo = Math.min(lo, v);
      hi = Math.max(hi, v);
    }
    approx(hi, 1, 1e-9);
    approx(lo, 0.3, 1e-6);
    approx(envelopeAt(env, 0), 1, 1e-9);
  });

  test("blink is lit for the duty fraction of its period", () => {
    const env: Envelope = { kind: "blink", period_ms: 1000, duty: 0.25 };
    expect(envelopeAt(env, 100)).toBe(1);
    expect(envelopeAt(env, 300)).toBe(0);
  });

  test("a pulse rises, holds, decays and then stops after its repeats", () => {
    const env: Envelope = { kind: "pulse", attack_ms: 100, hold_ms: 100, decay_ms: 200, repeat: 1 };
    approx(envelopeAt(env, 50), 0.5);
    expect(envelopeAt(env, 150)).toBe(1);
    approx(envelopeAt(env, 300), 0.25); // (1 - 0.5)^2
    expect(envelopeAt(env, 400)).toBe(0); // the one run is over
  });
});

describe("blend modes", () => {
  test("over: the layer covers what is below by its own largest channel", () => {
    const below: [number, number, number][] = [[0.5, 0.5, 0.5]];
    const layer: [number, number, number][] = [[0.8, 0.2, 0]];
    const out = blend(below, layer, "over");
    approx(out[0][0], 0.5 * (1 - 0.8) + 0.8);
    approx(out[0][1], 0.5 * (1 - 0.8) + 0.2);
    approx(out[0][2], 0.5 * (1 - 0.8) + 0);
  });

  test("max keeps the brighter channel, add sums and saturates", () => {
    const below = Array.from({ length: 16 }, () => [0.6, 0.2, 0] as [number, number, number]);
    const layer = Array.from({ length: 16 }, () => [0.5, 0.5, 0] as [number, number, number]);
    expect(blend(below, layer, "max")[0]).toEqual([0.6, 0.5, 0]);
    expect(blend(below, layer, "add")[0]).toEqual([1, 0.7, 0]);
  });

  test("a layer's brightness scales that layer alone", () => {
    const fill: Layer = { block: "fill", color: "ffffff", brightness: 0.25 };
    approx(renderLayer(fill, 0, 0)[0][0], 0.25);
  });
});

describe("looks", () => {
  test("a look's brightness scales the whole look", () => {
    const solid: Look = { layers: [{ block: "fill", color: "ffffff" }], brightness: 0.7 };
    approx(renderSpec(solid, 0)[5][1], 0.7);
  });

  test("a name not in the table renders nothing, even the Object prototype's names", () => {
    expect(renderLook("helios.missing", 0)).toBeNull();
    expect(renderLook("constructor", 0)).toBeNull();
  });

  test("a named look renders sixteen colours", () => {
    const frame = renderLook("helios.searching", 500)!;
    expect(frame.length).toBe(16);
    for (const c of frame) for (const v of c) expect(v >= 0 && v <= 1).toBe(true);
  });

  test("the colour parser reads the TOML's hex", () => {
    expect(rgbOf("28c8ff")).toEqual([40 / 255, 200 / 255, 255 / 255]);
  });
});

describe("ring geometry and easing", () => {
  test("ringDistance takes the shorter way round", () => {
    expect(ringDistance(15, 1)).toBe(2);
    expect(ringDistance(0, 8)).toBe(8);
  });

  test("approach closes about 63% of the gap each tau, whatever the frame time", () => {
    approx(approach(0, 1, 100, 100), 1 - Math.exp(-1));
    approx(approach(0, 1, 50, 100), 1 - Math.exp(-0.5));
  });
});

describe("spots block", () => {
  test("a spot lights its LED brightest and fades to its neighbours", () => {
    const layer: Layer = { block: "spots", color: "ffffff", width: 1.5 };
    const frame = renderLayer(layer, 0, 0, [{ at: 4, level: 1 }]);
    const brightest = frame.reduce((best, c, i) => (c[0] > frame[best][0] ? i : best), 0);
    expect(brightest).toBe(4);
    expect(frame[5][0]).toBeLessThan(frame[4][0]);
    expect(renderLayer(layer, 0, 0)[4][0]).toBe(0);
  });
});
