// Live device or simulated robot. The UI talks to helios-api unless mocks are
// asked for: `?mock=1` in the URL (remembered for the tab; `?mock=0` clears
// it) or VITE_HELIOS_MOCK=1 at build/dev time. The API base defaults to the
// page's own origin; VITE_HELIOS_API points the UI at another device.

const KEY = "helios.mock";

function detectMock(): boolean {
  if (import.meta.env.VITE_HELIOS_MOCK === "1") return true;
  if (typeof window === "undefined") return false;
  try {
    const param = new URLSearchParams(window.location.search).get("mock");
    if (param === "1" || param === "true") sessionStorage.setItem(KEY, "1");
    if (param === "0" || param === "false") sessionStorage.removeItem(KEY);
    return sessionStorage.getItem(KEY) === "1";
  } catch {
    return false;
  }
}

export const MOCK: boolean = detectMock();

export const API_BASE: string = (import.meta.env.VITE_HELIOS_API ?? "").replace(/\/$/, "");
