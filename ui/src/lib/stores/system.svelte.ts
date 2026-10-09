// Per-device internals for the deep views: cores, processes, fan, LEDs, GPIO,
// IMU and power rails. Live: cores and processes come from helios-api; fan,
// LEDs, IMU and power rails have no device backend yet and stay empty.
// With `?mock=1` everything is simulated next to the mock cluster.

import { api, errorText, isNotAvailable } from "#lib/api/client.js";
import { MOCK } from "#lib/api/mode.js";
import type * as W from "#lib/api/types.js";
import { cluster } from "./cluster.svelte";
import { toasts } from "./toasts.svelte";

export interface Proc {
  pid: number;
  nodeId: string;
  name: string;
  /** What owns it: a system service, a pipeline (workload id) or the kernel. */
  owner: { kind: "service" | "workload" | "kernel" | "user"; id: string };
  state: "running" | "sleeping" | "stopped" | "zombie";
  /** Fraction of one core. */
  cpu: number;
  memMiB: number;
  threads: number;
  /** Pinned core, or null for any. */
  core: number | null;
  nice: number;
  startedAt: number;
}

export interface FanState {
  mode: "auto" | "manual";
  duty: number;
  rpm: number;
  /** Temperature (°C) -> duty (0..1) points, ascending. */
  curve: [number, number][];
}

export interface LedState {
  id: string;
  name: string;
  count: number;
  color: string;
  pattern: "solid" | "blink" | "breathe" | "status";
  brightness: number;
}

export interface GpioPin {
  pin: number;
  name: string;
  mode: "in" | "out" | "pwm" | "alt";
  value: number;
  owner: string | null;
}

export interface DeviceHw {
  cores: number[];
  coreHistory: number[][];
  freqMHz: number[];
  procs: Proc[];
  fan: FanState | null;
  leds: LedState[];
  gpio: GpioPin[];
  imu: {
    yaw: number;
    pitch: number;
    roll: number;
    rateHz: number;
    gyroRange: number;
    accelRange: number;
    lpfHz: number;
    /** IMU orientation on the robot: roll, pitch, yaw (°). */
    mount: [number, number, number];
    gyroBias: [number, number, number];
    temperatureC: number;
  } | null;
  power: { rail: string; volts: number; amps: number }[];
  throttled: boolean;
}

const CORES: Record<string, number> = { raze: 4, mcu: 2, foreign: 8, coprocessor: 8 };

function jitter(v: number, s: number, min = 0, max = Infinity) {
  return Math.min(max, Math.max(min, v + (Math.random() - 0.5) * s));
}

let pidSeq = 200;

function procsFor(nodeId: string): Proc[] {
  const node = cluster.node(nodeId)!;
  const t = Date.now() - node.uptimeS * 1000;
  const list: Proc[] = [];
  const add = (name: string, owner: Proc["owner"], cpu: number, memMiB: number, threads = 1, core: number | null = null, nice = 0) =>
    list.push({ pid: (pidSeq += 1 + Math.floor(Math.random() * 30)), nodeId, name, owner, state: "running", cpu, memMiB, threads, core, nice, startedAt: t + Math.random() * 4000 });
  if (node.kind === "mcu") {
    add("scheduler", { kind: "kernel", id: "rtos" }, 0.02, 0.01);
    for (const s of node.services) add(s.name, { kind: "service", id: s.name }, node.cpu * 0.8, s.memMiB, 1, 1);
    add("agent", { kind: "service", id: "atlas-agent" }, 0.01, 0.02, 1, 0);
    return list;
  }
  add("init", { kind: "kernel", id: "init" }, 0.0005, 1.1);
  add("kworker/u8:1", { kind: "kernel", id: "kernel" }, 0.004, 0);
  for (const s of node.services) {
    const busy = s.name === "helios-io" ? 0.06 : s.name === "orion-node" ? 0.012 : 0.004;
    add(s.name, { kind: "service", id: s.name }, busy, s.memMiB, s.name === "orion-node" ? 4 : 2, s.name === "helios-io" ? 2 : null, s.name === "atlas-agent" ? -5 : 0);
  }
  for (const w of cluster.workloadsOn(nodeId)) {
    if (w.state === "stopped") continue;
    add(`runner:${w.name}`, { kind: "workload", id: w.id }, w.state === "running" ? (w.perf.tickP50Ms * w.perf.fps) / 1000 : 0, w.perf.memMiB, 1, 3, -2);
  }
  if (node.kind !== "raze") add("chrome", { kind: "user", id: "user" }, 0.06, 420, 24);
  return list;
}

function defaultHw(nodeId: string): DeviceHw {
  const node = cluster.node(nodeId)!;
  const n = CORES[node.kind] ?? 4;
  const cores = Array.from({ length: n }, (_, i) => (i === 3 && node.kind === "raze" ? 0.06 : node.cpu * (0.6 + Math.random() * 0.8)));
  const raze = node.kind === "raze";
  return {
    cores,
    coreHistory: cores.map((c) => Array.from({ length: 40 }, () => jitter(c, c * 0.4))),
    freqMHz: cores.map(() => (raze ? 2400 : node.kind === "mcu" ? 150 : 3200)),
    procs: procsFor(nodeId),
    fan: raze
      ? { mode: "auto", duty: 0.32, rpm: 3100, curve: [[40, 0.2], [55, 0.35], [65, 0.6], [75, 1]] }
      : null,
    leds: raze
      ? [{ id: "status", name: "Status LED", count: 1, color: "#22c55e", pattern: "status", brightness: 0.6 }]
      : nodeId === "mcu-leds"
        ? [
            { id: "strip-a", name: "Strip A · underglow", count: 60, color: "#ff6b6b", pattern: "breathe", brightness: 0.8 },
            { id: "strip-b", name: "Strip B · intake", count: 24, color: "#4dabf7", pattern: "solid", brightness: 0.7 },
            { id: "strip-c", name: "Strip C · turret", count: 12, color: "#ffd43b", pattern: "status", brightness: 1 },
            { id: "strip-d", name: "Strip D · rear", count: 30, color: "#da77f2", pattern: "blink", brightness: 0.5 },
          ]
        : [],
    gpio: raze
      ? [
          { pin: 4, name: "FLASH_SYNC", mode: "out", value: 0, owner: "helios-io" },
          { pin: 17, name: "IR_LED", mode: "pwm", value: 0.4, owner: null },
          { pin: 22, name: "BTN_RESET", mode: "in", value: 1, owner: "atlas-agent" },
          { pin: 23, name: "AUX_1", mode: "in", value: 0, owner: null },
          { pin: 24, name: "AUX_2", mode: "out", value: 1, owner: null },
          { pin: 27, name: "TRIGGER", mode: "out", value: 0, owner: "helios-io" },
        ]
      : [],
    imu: nodeId === "mcu-imu" ? { yaw: 12.5, pitch: 0.4, roll: -0.2, rateHz: 1000, gyroRange: 2000, accelRange: 24, lpfHz: 116, mount: [0, 0, 0], gyroBias: [0.012, -0.004, 0.021], temperatureC: 38.5 } : null,
    power: raze
      ? [
          { rail: "5V in", volts: 5.08, amps: 1.42 },
          { rail: "3V3", volts: 3.31, amps: 0.21 },
          { rail: "VDD_CORE", volts: 0.88, amps: 2.1 },
        ]
      : nodeId.startsWith("mcu")
        ? [{ rail: "5V CAN", volts: 5.02, amps: 0.08 }]
        : [],
    throttled: false,
  };
}

function procState(state: string): Proc["state"] {
  if (state === "R") return "running";
  if (state === "T" || state === "t") return "stopped";
  if (state === "Z") return "zombie";
  return "sleeping";
}

function singleCore(list: string | null): number | null {
  return list && /^\d+$/.test(list) ? Number(list) : null;
}

class SystemStore {
  hw = $state<Record<string, DeviceHw>>({});
  private timer = 0;
  private lastCpu = new Map<number, { ms: number; at: number }>();
  private warned = new Set<string>();

  private async liveTick() {
    const node = cluster.nodes[0];
    if (!node) return;
    let processes: W.ProcessInfo[] = [];
    let metrics: W.Metrics | null = null;
    try {
      [processes, metrics] = await Promise.all([api.processes(), api.metrics()]);
    } catch {
      return;
    }
    const now = Date.now();
    const bootAt = now - (metrics.uptime_s ?? node.uptimeS) * 1000;
    const procs: Proc[] = processes.map((p) => {
      const last = this.lastCpu.get(p.pid);
      const cpu = last && now > last.at ? Math.max(0, (p.cpu_time_ms - last.ms) / (now - last.at)) : 0;
      this.lastCpu.set(p.pid, { ms: p.cpu_time_ms, at: now });
      const unit = p.unit?.replace(/\.service$/, "");
      return {
        pid: p.pid,
        nodeId: node.id,
        name: p.name,
        owner: unit ? { kind: "service", id: unit } : p.name.startsWith("kworker") || p.rss_bytes === 0 ? { kind: "kernel", id: "kernel" } : { kind: "user", id: "user" },
        state: procState(p.state),
        cpu,
        memMiB: p.rss_bytes / 1024 / 1024,
        threads: p.threads,
        core: singleCore(p.cpus_allowed),
        nice: p.nice,
        startedAt: bootAt + p.started_after_boot_ms,
      };
    });
    const prev = this.hw[node.id];
    const cores = metrics.cpu_cores;
    this.hw[node.id] = {
      cores,
      coreHistory: cores.map((c, i) => [...(prev?.coreHistory[i] ?? []), c].slice(-40)),
      freqMHz: cores.map(() => 0),
      procs,
      fan: null,
      leds: [],
      gpio: [],
      imu: null,
      power: [],
      throttled: metrics.throttled != null && (metrics.throttled & 0x4) !== 0,
    };
  }

  private async liveAct(feature: string, run: () => Promise<unknown>, success?: string) {
    try {
      await run();
      if (success) toasts.success(success);
    } catch (error) {
      if (isNotAvailable(error)) {
        if (!this.warned.has(feature)) {
          this.warned.add(feature);
          toasts.info(`${error.message}. Not available on this device yet.`);
        }
      } else toasts.error(`${feature}: ${errorText(error)}`);
    }
    void this.liveTick();
  }

  start() {
    if (this.timer || typeof window === "undefined") return () => {};
    if (!MOCK) {
      void this.liveTick();
      this.timer = window.setInterval(() => void this.liveTick(), 2000);
      return () => {
        clearInterval(this.timer);
        this.timer = 0;
      };
    }
    for (const n of cluster.nodes) this.hw[n.id] = defaultHw(n.id);
    this.timer = window.setInterval(() => this.tick(), 1000);
    return () => {
      clearInterval(this.timer);
      this.timer = 0;
    };
  }

  of(nodeId: string): DeviceHw | undefined {
    return this.hw[nodeId];
  }

  get allProcs(): Proc[] {
    return Object.values(this.hw).flatMap((h) => h.procs);
  }

  private tick() {
    for (const n of cluster.nodes) {
      const h = this.hw[n.id];
      if (!h) continue;
      const off = n.health === "offline";
      h.cores = h.cores.map((c, i) => (off ? 0 : jitter(c, Math.max(0.01, c * 0.2), 0.002, 1)));
      h.cores.forEach((c, i) => {
        h.coreHistory[i].push(c);
        if (h.coreHistory[i].length > 40) h.coreHistory[i].shift();
      });
      for (const p of h.procs) {
        if (p.state !== "running") continue;
        if (p.owner.kind === "workload") {
          const w = cluster.workload(p.owner.id);
          p.cpu = w && w.state === "running" ? (w.perf.tickP50Ms * w.perf.fps) / 1000 : 0;
        } else p.cpu = jitter(p.cpu, p.cpu * 0.2, 0.0001);
      }
      if (h.fan) {
        if (h.fan.mode === "auto") h.fan.duty = this.curveDuty(h.fan.curve, n.tempC);
        h.fan.rpm = Math.round(jitter(h.fan.duty * 9000 + 300, 60));
      }
      if (h.imu) {
        h.imu.yaw = (h.imu.yaw + (Math.random() - 0.48) * 1.5 + 360) % 360;
        h.imu.pitch = jitter(h.imu.pitch, 0.1);
        h.imu.roll = jitter(h.imu.roll, 0.1);
      }
      for (const r of h.power) r.amps = jitter(r.amps, r.amps * 0.05, 0);
    }
  }

  curveDuty(curve: [number, number][], t: number): number {
    if (!curve.length) return 0.5;
    if (t <= curve[0][0]) return curve[0][1];
    for (let i = 1; i < curve.length; i++) {
      const [t1, d1] = curve[i];
      const [t0, d0] = curve[i - 1];
      if (t <= t1) return d0 + ((t - t0) / (t1 - t0)) * (d1 - d0);
    }
    return curve.at(-1)![1];
  }

  // --- Actions -----------------------------------------------------------

  async kill(nodeId: string, pid: number, signal: "TERM" | "KILL" = "TERM") {
    if (!MOCK) return this.liveAct("Signal", () => api.signal(pid, signal), `SIG${signal} sent to ${pid}`);
    const h = this.hw[nodeId];
    const p = h?.procs.find((x) => x.pid === pid);
    if (!h || !p) return;
    if (p.owner.kind === "workload") {
      await cluster.stopWorkload(p.owner.id);
      h.procs = h.procs.filter((x) => x !== p);
      return;
    }
    h.procs = h.procs.filter((x) => x !== p);
    toasts.info(`SIG${signal} sent to ${p.name} (${p.pid})`);
    if (p.owner.kind === "service") {
      // Supervised: the service comes back with a new pid.
      await new Promise((r) => setTimeout(r, 900));
      h.procs.push({ ...p, pid: (pidSeq += 7), startedAt: Date.now(), cpu: p.cpu });
      const s = cluster.node(nodeId)?.services.find((x) => x.name === p.owner.id);
      if (s) s.restarts += 1;
      toasts.success(`${p.name} restarted by its supervisor`);
    }
  }

  setState(nodeId: string, pid: number, state: "running" | "stopped") {
    if (!MOCK) {
      void this.liveAct("Signal", () => api.signal(pid, state === "stopped" ? "STOP" : "CONT"), `${pid} ${state === "stopped" ? "paused (SIGSTOP)" : "resumed (SIGCONT)"}`);
      return;
    }
    const p = this.hw[nodeId]?.procs.find((x) => x.pid === pid);
    if (!p) return;
    p.state = state;
    if (state === "stopped") p.cpu = 0;
    toasts.info(`${p.name} ${state === "stopped" ? "paused (SIGSTOP)" : "resumed (SIGCONT)"}`);
  }

  pin(nodeId: string, pid: number, core: number | null) {
    if (!MOCK) {
      void this.liveAct("CPU affinity", () => api.setAffinity(pid, core));
      return;
    }
    const p = this.hw[nodeId]?.procs.find((x) => x.pid === pid);
    if (!p) return;
    p.core = core;
    toasts.success(`${p.name} ${core === null ? "may run on any core" : `pinned to core ${core}`}`);
  }

  async calibrateGyro(nodeId: string) {
    if (!MOCK) return this.liveAct("IMU", () => Promise.reject(new Error("no IMU on this device")));
    const imu = this.hw[nodeId]?.imu;
    if (!imu) return;
    toasts.info("Calibrating gyro: keep the robot still for 3 s");
    await new Promise((r) => setTimeout(r, 3000));
    imu.gyroBias = imu.gyroBias.map(() => +((Math.random() - 0.5) * 0.002).toFixed(4)) as [number, number, number];
    toasts.success("Gyro bias updated");
  }

  renice(nodeId: string, pid: number, nice: number) {
    if (!MOCK) {
      void this.liveAct("Priority", () => api.setNice(pid, nice));
      return;
    }
    const p = this.hw[nodeId]?.procs.find((x) => x.pid === pid);
    if (p) p.nice = Math.max(-20, Math.min(19, nice));
  }
}

export const system = new SystemStore();
