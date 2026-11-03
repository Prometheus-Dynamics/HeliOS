// The UI's own failure log. Uncaught errors, unhandled promise rejections and failed API calls are
// reported to the device (`POST /v1/logs/client`), with the last 20 user actions before each, so the
// device's journal says what failed, where, and after which actions. Nobody has to ask the user.
//
// Never recorded: typed text, field values, passwords, tokens and request bodies. A click records the
// clicked control's name (its data-testid, aria-label, or visible text) and a route change its hash.
//
// Sending is batched and best effort: no reply is waited for, a failed send is dropped (never retried,
// and never reported), nothing is sent while the browser is offline, and the simulated robot (`?mock=1`)
// has no device to send to.

import { API_BASE, MOCK } from "./mode";

/** The user actions kept before an error, the last ones. */
const CRUMBS = 20;
const SEND_AFTER_MS = 1500;
/** Reports per request: the server takes up to 20 and 96 KiB; ten of these stay well under both. */
const BATCH = 10;
/** Reports waiting to be sent; past it the oldest is dropped (and counted). */
const QUEUE_MAX = 50;
/** Reports this browser sends a minute; the server allows 120 from every client together. */
const PER_MINUTE = 30;
/** The body of a failed API answer, as far as the report goes (bytes). */
const BODY_BYTES = 512;
/** One toast per message for this long: a render loop must not fill the screen. */
const TOAST_EVERY_MS = 5000;

/** Clickable things: the breadcrumb records a click on one of these (and only these). */
const CONTROL = [
  "button",
  "a[href]",
  "summary",
  "input[type=checkbox]",
  "input[type=radio]",
  "input[type=button]",
  "input[type=submit]",
  "[role=button]",
  "[role=link]",
  "[role=tab]",
  "[role=menuitem]",
  "[role=menuitemcheckbox]",
  "[role=menuitemradio]",
  "[role=option]",
  "[role=switch]",
  "[role=checkbox]",
  "[role=radio]",
  "[role=treeitem]",
  "[data-testid]",
].join(", ");

export type ReportKind = "error" | "rejection" | "api";

/** A report as the device takes it (`backend/.../routes/logs.rs`, `ClientReport`). */
export interface ClientReport {
  id: string;
  kind: ReportKind;
  at_ms: number;
  message?: string;
  stack?: string;
  source?: string;
  route?: string;
  method?: string;
  path?: string;
  status?: number;
  body?: string;
  after: string[];
}

/** What a caller knows about a failure; the id, time, route and the actions before are added here. */
export type FailureInput = Omit<ClientReport, "id" | "at_ms" | "route" | "after">;

const crumbs: string[] = [];
let lastRoute = "";
let csrf: () => string | null = () => null;
let queue: ClientReport[] = [];
let dropped = 0;
let timer: ReturnType<typeof setTimeout> | null = null;
let windowStart = 0;
let windowCount = 0;
let lastToast = { message: "", at: 0 };
let started = false;
/** Shows an error to the user (the layout passes the toast store's; none until it does). */
let showError: (message: string) => void = () => {};

/** The session's CSRF token, which a report carries like any other mutation (`client.ts`). */
export function setReportCsrf(source: () => string | null) {
  csrf = source;
}

function clock(): string {
  return new Date().toISOString().slice(11, 19);
}

function clip(text: string, max: number): string {
  const flat = text.replace(/\s+/g, " ").trim();
  return flat.length <= max ? flat : `${flat.slice(0, max - 3)}...`;
}

function addCrumb(text: string) {
  crumbs.push(`${clock()} ${text}`);
  if (crumbs.length > CRUMBS) crumbs.splice(0, crumbs.length - CRUMBS);
}

function currentRoute(): string {
  return typeof location === "undefined" ? "" : location.hash || "#";
}

/** A screen change, kept as one breadcrumb (repeats of the same screen are not new actions). */
export function breadcrumbRoute(screen: string) {
  const route = `#${screen.replace(/^#/, "")}`;
  if (route === lastRoute) return;
  lastRoute = route;
  addCrumb(`route ${route}`);
}

/** A short id for the user to quote, e.g. `ui-3f9a1c2e`. */
function newId(): string {
  const bytes = new Uint8Array(4);
  if (typeof crypto !== "undefined" && crypto.getRandomValues) crypto.getRandomValues(bytes);
  else for (let i = 0; i < bytes.length; i++) bytes[i] = Math.floor(Math.random() * 256);
  return `ui-${Array.from(bytes, (b) => b.toString(16).padStart(2, "0")).join("")}`;
}

/** The first bytes of a failed answer's body, cut so that the excerpt stays within `BODY_BYTES`. */
export function excerpt(text: string): string {
  const bytes = new TextEncoder().encode(text);
  if (bytes.length <= BODY_BYTES) return text;
  // A character cut in two decodes as one replacement character at the end, which goes too.
  return `${new TextDecoder().decode(bytes.slice(0, BODY_BYTES - 3)).replace(/\uFFFD$/, "")}...`;
}

function stackLines(stack: string | undefined): string | undefined {
  if (!stack) return undefined;
  return clip(stack.split("\n").slice(0, 8).join("\n"), 800);
}

function offline(): boolean {
  return typeof navigator !== "undefined" && navigator.onLine === false;
}

function admit(): boolean {
  const now = Date.now();
  if (now - windowStart >= 60_000) {
    windowStart = now;
    windowCount = 0;
  }
  if (windowCount >= PER_MINUTE) return false;
  windowCount++;
  return true;
}

/**
 * Records a failure and queues it for the device. Returns its id at once, for the user to quote:
 * the id is shown even when nothing is sent (offline, the simulated robot, or over the rate limit).
 */
export function reportFailure(failure: FailureInput): string {
  const id = newId();
  if (MOCK || offline()) return id;
  if (!admit()) {
    dropped++;
    return id;
  }
  const report: ClientReport = {
    ...failure,
    message: failure.message ? clip(failure.message, 400) : undefined,
    stack: stackLines(failure.stack),
    body: failure.body ? excerpt(failure.body) : undefined,
    id,
    at_ms: Date.now(),
    route: currentRoute(),
    after: crumbs.slice(),
  };
  queue.push(report);
  if (queue.length > QUEUE_MAX) {
    queue.shift();
    dropped++;
  }
  schedule();
  return id;
}

function schedule() {
  if (timer === null) timer = setTimeout(() => void flush(), SEND_AFTER_MS);
}

/** Sends up to one batch. A failed send is dropped: no retry, and no report of the report. */
async function flush() {
  timer = null;
  if (offline()) {
    queue = [];
    dropped = 0;
    return;
  }
  if (queue.length === 0 && dropped === 0) return;
  const batch = queue.splice(0, BATCH);
  const count = dropped;
  dropped = 0;
  try {
    const headers: Record<string, string> = { "Content-Type": "application/json", Accept: "application/json" };
    const token = csrf();
    if (token) headers["X-Helios-CSRF"] = token;
    await fetch(`${API_BASE}/v1/logs/client`, {
      method: "POST",
      headers,
      body: JSON.stringify({ reports: batch, dropped: count }),
      credentials: "same-origin",
      keepalive: true,
    });
  } catch {
    // The device is unreachable or refused the batch: nothing else to do, and no retry loop.
  }
  if (queue.length > 0) schedule();
}

/** Sends what is queued now (the page is being hidden, so the timer may not run). */
function flushNow() {
  if (timer !== null) clearTimeout(timer);
  void flush();
}

/** One toast per message for a while, with the id the device has the report under. */
function toastOnce(message: string, id: string) {
  const now = Date.now();
  if (message === lastToast.message && now - lastToast.at < TOAST_EVERY_MS) return;
  lastToast = { message, at: now };
  showError(`${message} (ref ${id})`);
}

function reasonText(reason: unknown): string {
  if (reason instanceof Error) return reason.message;
  if (typeof reason === "string") return reason;
  try {
    return JSON.stringify(reason) ?? String(reason);
  } catch {
    return String(reason);
  }
}

/** The accessible name a user would read for a control: its test id, label, title or visible text. */
function controlName(element: Element): string {
  const testId = element.getAttribute("data-testid");
  if (testId) return `#${testId}`;
  const label = element.getAttribute("aria-label");
  if (label) return clip(label, 60);
  const labelledBy = element.getAttribute("aria-labelledby");
  if (labelledBy) {
    const text = labelledBy
      .split(/\s+/)
      .map((id) => document.getElementById(id)?.textContent ?? "")
      .join(" ");
    if (text.trim()) return clip(text, 60);
  }
  const title = element.getAttribute("title");
  if (title) return clip(title, 60);
  return clip(element.textContent ?? "", 60) || element.tagName.toLowerCase();
}

function describe(element: Element): string {
  const role = element.getAttribute("role") ?? element.tagName.toLowerCase();
  return `${role} "${controlName(element)}"`;
}

/**
 * Starts the capture: errors, rejections, route changes and clicks. `show` puts an error in front of
 * the user (a toast with the report's id). Returns the stop function. Idempotent while running.
 */
export function startClientLog(show: (message: string) => void = () => {}): () => void {
  if (started || typeof window === "undefined") return () => {};
  started = true;
  showError = show;

  const onError = (event: ErrorEvent) => {
    const error = event.error;
    const message = error instanceof Error ? error.message : event.message || "Uncaught error";
    const stack = error instanceof Error ? error.stack : undefined;
    const source = event.filename ? `${event.filename}:${event.lineno}:${event.colno}` : undefined;
    const id = reportFailure({ kind: "error", message, stack, source });
    toastOnce("Unexpected error", id);
  };
  const onRejection = (event: PromiseRejectionEvent) => {
    const reason = event.reason;
    const message = reasonText(reason);
    const stack = reason instanceof Error ? reason.stack : undefined;
    const id = reportFailure({ kind: "rejection", message, stack });
    toastOnce("Unexpected error", id);
  };
  const onClick = (event: MouseEvent) => {
    const target = event.target instanceof Element ? event.target : null;
    const control = target?.closest(CONTROL) ?? null;
    if (control) addCrumb(`click ${describe(control)}`);
  };
  const onHash = () => breadcrumbRoute(location.hash.slice(1));
  const onHidden = () => {
    if (document.visibilityState === "hidden") flushNow();
  };

  window.addEventListener("error", onError);
  window.addEventListener("unhandledrejection", onRejection);
  window.addEventListener("hashchange", onHash);
  document.addEventListener("click", onClick, true);
  document.addEventListener("visibilitychange", onHidden);
  if (location.hash) breadcrumbRoute(location.hash.slice(1));

  return () => {
    window.removeEventListener("error", onError);
    window.removeEventListener("unhandledrejection", onRejection);
    window.removeEventListener("hashchange", onHash);
    document.removeEventListener("click", onClick, true);
    document.removeEventListener("visibilitychange", onHidden);
    started = false;
  };
}

/** The breadcrumbs as they stand (for tests and for a report made elsewhere). */
export function breadcrumbs(): readonly string[] {
  return crumbs;
}

/** Test support: the queue, the drop count and the rate window, back to empty. */
export function resetClientLogForTests() {
  crumbs.length = 0;
  lastRoute = "";
  queue = [];
  dropped = 0;
  windowStart = 0;
  windowCount = 0;
  lastToast = { message: "", at: 0 };
  if (timer !== null) clearTimeout(timer);
  timer = null;
}

/** Test support: what is queued, waiting to be sent. */
export function queuedReports(): readonly ClientReport[] {
  return queue;
}
