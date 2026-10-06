// Help: the first-run setup, the guided tour, and "what's on this screen".

import { loadRaw, save } from "$lib/core/persist";

export interface TourStep {
  /** CSS selector of the element to point at (first match). */
  target: string;
  title: string;
  body: string;
}

export const TOUR: TourStep[] = [
  { target: "[data-tour=rail]", title: "Screens", body: "Each icon is a screen: a layout of panes for one job. Number keys 1–9 switch between them. Right-click one to rename, recolour, reset or hide it." },
  { target: "[data-tour=add-screen]", title: "Add more when you need it", body: "Start is deliberately simple. Add screens like Cameras, Pipelines or Hardware from here, or drop prebuilt kits of panes into any screen." },
  { target: ".stack .tabs", title: "Panes and tabs", body: "Every box is a pane; its tab names it (hover for what it does). Drag a tab to move it or onto an edge to split. Its tools sit in the same row. ⋮ adds panes, splits, and reopens closed ones." },
  { target: "[data-tour=readouts]", title: "The robot at a glance", body: "Devices online, pipelines running, slowest tick, tags in view, hottest device and clock sync. Click any of them to go to the screen that explains it." },
  { target: "[data-tour=security]", title: "Open or secured", body: "Open means anyone on this network can change the device, which is normal on an FRC robot. Click it to secure the device with a password, change the password, or make API tokens for Atlas and scripts." },
  { target: "[data-tour=search]", title: "Search, or describe a problem", body: "Ctrl K finds any screen, pane, camera or setting. You can also type a problem, like “image too dark” or “calibrate”, and it takes you to the fix." },
  { target: "[data-tour=status]", title: "Status bar", body: "What is selected, with its key numbers; how many problems there are; and the next setup step. Click a problem count to see the fixes." },
  { target: "[data-tour=help]", title: "Help is here", body: "“What's on this screen?” labels every pane. You can replay this tour or the first-time setup any time." },
];

class HelpStore {
  onboarding = $state(!loadRaw("onboarded", false));
  explain = $state(false);
  tourStep = $state<number | null>(null);

  finishOnboarding() {
    this.onboarding = false;
    save("onboarded", true);
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
