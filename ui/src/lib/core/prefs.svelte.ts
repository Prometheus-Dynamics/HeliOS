// Look and feel: theme, density and an optional accent override.

import { load, save } from "./persist";

export const THEMES = [
  { id: "glass", name: "Glass", note: "Atlas's dark glass" },
  { id: "graphite", name: "Graphite", note: "flat, dense, amber" },
  { id: "midnight", name: "Midnight", note: "true black, blue" },
  { id: "ember", name: "Ember", note: "warm dark" },
  { id: "daylight", name: "Daylight", note: "light" },
  { id: "field", name: "Field", note: "max contrast for pits and sunlight" },
] as const;

export type ThemeId = (typeof THEMES)[number]["id"];

interface Prefs {
  theme: ThemeId;
  density: "compact" | "comfortable";
  accent: string | null;
}

class PrefsStore {
  #value = $state<Prefs>(load("prefs", { theme: "glass", density: "compact", accent: null }));

  get theme() {
    return this.#value.theme;
  }
  set theme(theme: ThemeId) {
    this.#value.theme = theme;
    this.apply();
  }
  get density() {
    return this.#value.density;
  }
  set density(density: Prefs["density"]) {
    this.#value.density = density;
    this.apply();
  }
  get accent() {
    return this.#value.accent;
  }
  set accent(accent: string | null) {
    this.#value.accent = accent;
    this.apply();
  }

  apply() {
    if (typeof document === "undefined") return;
    const root = document.documentElement;
    root.dataset.theme = this.#value.theme;
    root.dataset.density = this.#value.density;
    const light = this.#value.theme === "daylight" || this.#value.theme === "field";
    root.classList.toggle("dark", !light);
    if (this.#value.accent) {
      root.style.setProperty("--accent", this.#value.accent);
      root.style.setProperty("--accent-fg", this.#value.accent);
    } else {
      root.style.removeProperty("--accent");
      root.style.removeProperty("--accent-fg");
    }
    save("prefs", this.#value);
  }
}

export const prefs = new PrefsStore();
