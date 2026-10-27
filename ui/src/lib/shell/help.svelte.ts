// Help: the first-run setup, the guided tour, and "what's on this screen".

import { setup } from "#lib/core/setup.svelte.js";
import { onSettingsChange } from "#lib/core/sync.js";

export interface TourStep {
  /** CSS selector of the element to point at (first match). */
  target: string;
  title: string;
  body: string;
}

export const TOUR: TourStep[] = [
  { target: "[data-tour=rail]", title: "Four jobs", body: "Set up (first run, a new camera), Run (match time, nothing to edit), Tune (one camera or pipeline at a time) and Device (boards, updates, logs, recording). Keys 1–4 switch. Every control is on exactly one of them." },
  { target: "[data-tour=add-screen]", title: "Screens of your own", body: "Add a screen under the four jobs: a copy of one, or blank with kits of panes. Right-click one to rename, recolour or delete it." },
  { target: ".stack .tabs", title: "Panes and tabs", body: "Every box is a pane; its tab names it (hover for what it does). Drag a tab to move it or onto an edge to split. Its tools sit in the same row. ⋮ adds panes, splits, and reopens closed ones." },
  { target: "[data-tour=readouts]", title: "The robot at a glance", body: "Devices online, pipelines running, slowest tick, tags in view, hottest device and clock sync. Click any of them to go to the screen that explains it." },
  { target: "[data-tour=security]", title: "Open or secured", body: "Open means anyone on this network can change the device, which is normal on an FRC robot. Click it to secure the device with a password, change the password, or make API tokens for Atlas and scripts." },
  { target: "[data-tour=search]", title: "Search, or describe a problem", body: "Ctrl K finds any screen, pane, camera or setting. You can also type a problem, like “image too dark” or “calibrate”, and it takes you to the fix." },
  { target: "[data-tour=status]", title: "Status bar", body: "What is selected, with its key numbers; how many problems there are (click for the fixes, on Run); and the next setup step (click for the checklist, on Set up)." },
  { target: "[data-tour=help]", title: "Help is here", body: "“What's on this screen?” labels every pane. You can replay this tour or the first-time setup any time." },
];

class HelpStore {
  /** First-time setup is showing. Follows the device (see `followSetup`). */
  onboarding = $state(false);
  explain = $state(false);
  tourStep = $state<number | null>(null);
  #followed = false;

  /** Show first-time setup until the device says it is done, and follow it on every tab. */
  followSetup() {
    if (this.#followed) return;
    this.#followed = true;
    onSettingsChange("setup", () => void this.#syncSetup());
    void this.#syncSetup();
  }

  async #syncSetup() {
    this.onboarding = !(await setup.load());
  }

  finishOnboarding() {
    this.onboarding = false;
    void setup.complete();
  }

  startTour() {
    this.explain = false;
    this.tourStep = 0;
  }

  next() {
    if (this.tourStep === null) return;
    this.tourStep = this.tourStep + 1 < TOUR.length ? this.tourStep + 1 : null;
  }

  back() {
    if (this.tourStep) this.tourStep -= 1;
  }

  endTour() {
    this.tourStep = null;
  }
}

export const help = new HelpStore();
