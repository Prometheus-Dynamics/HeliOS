// Formatting shared by every screen.

export type Tone = "success" | "warning" | "error" | "primary" | "neutral" | "info";

export function timeAgo(ms: number, now = Date.now()): string {
  const s = Math.max(0, Math.round((now - ms) / 1000));
  if (s < 5) return "just now";
  if (s < 60) return `${s}s ago`;
  const m = Math.round(s / 60);
  if (m < 60) return `${m}m ago`;
  const h = Math.round(m / 60);
  if (h < 48) return `${h}h ago`;
  return `${Math.round(h / 24)}d ago`;
}

export function clockTime(ms: number): string {
  return new Date(ms).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", second: "2-digit" });
}

export function duration(seconds: number): string {
  if (seconds < 60) return `${Math.round(seconds)}s`;
  const m = Math.floor(seconds / 60);
  if (m < 60) return `${m}m ${Math.round(seconds % 60)}s`;
  const h = Math.floor(m / 60);
  if (h < 48) return `${h}h ${m % 60}m`;
  return `${Math.floor(h / 24)}d ${h % 24}h`;
}

export function bytes(n: number): string {
  const units = ["B", "KiB", "MiB", "GiB"];
  let value = n;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  return `${value < 10 && unit > 0 ? value.toFixed(1) : Math.round(value)} ${units[unit]}`;
}

/** Milliseconds with sensible precision for timings. */
export function ms(value: number): string {
  if (value < 0.1) return `${(value * 1000).toFixed(0)} µs`;
  if (value < 10) return `${value.toFixed(2)} ms`;
  return `${value.toFixed(1)} ms`;
}

export function pct(fraction: number): string {
  return `${Math.round(fraction * 100)}%`;
}

export function sentence(text: string): string {
  return text.length ? text[0].toUpperCase() + text.slice(1) : text;
}
