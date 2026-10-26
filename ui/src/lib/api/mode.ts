// Live device or simulated robot. The UI talks to helios-api unless mocks are
// asked for: `?mock=1` in the URL (remembered for the tab; `?mock=0` clears
// it) or VITE_HELIOS_MOCK=1 at build/dev time. The API base defaults to the
// page's own origin; VITE_HELIOS_API points the UI at another device.
//
// The simulated device is the Raze image by default (`?mock=1`, an owner with
// all its hardware). `?mock=systemcore` simulates a SystemCore guest: no
// updates, reboot, security or LED ring (see mock-devices.ts).

const KEY = "helios.mock";

/** The simulated devices: the Raze image (owner) and a SystemCore guest. */
export type MockDevice = "raze" | "systemcore";

/** The device a mock param or a stored choice names; `1` (the older form) is the Raze. */
function mockDevice(value: string | null): MockDevice | null {
  if (value === "1" || value === "true" || value === "raze") return "raze";
  return value === "systemcore" ? "systemcore" : null;
}

function detect(): { on: boolean; device: MockDevice } {
  if (import.meta.env.VITE_HELIOS_MOCK === "1") return { on: true, device: "raze" };
  if (typeof window === "undefined") return { on: false, device: "raze" };
  try {
    const param = new URLSearchParams(window.location.search).get("mock");
    if (param === "0" || param === "false") sessionStorage.removeItem(KEY);
    else if (param !== null && mockDevice(param)) sessionStorage.setItem(KEY, mockDevice(param)!);
    const stored = mockDevice(sessionStorage.getItem(KEY));
    return { on: stored !== null, device: stored ?? "raze" };
  } catch {
    return { on: false, device: "raze" };
  }
}

const detected = detect();

export const MOCK: boolean = detected.on;

/** Which simulated device runs when MOCK is on. */
export const MOCK_DEVICE: MockDevice = detected.device;

export const API_BASE: string = (import.meta.env.VITE_HELIOS_API ?? "").replace(/\/$/, "");
