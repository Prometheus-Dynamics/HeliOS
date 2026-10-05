// The team this robot belongs to: sets where robot code (NetworkTables) is.
import { loadRaw, save } from "./persist";

class TeamStore {
  #number = $state<number | null>(loadRaw<number | null>("team", null));

  get number() {
    return this.#number;
  }
  set number(n: number | null) {
    this.#number = n;
    save("team", n);
  }

  /** The roboRIO address WPILib uses for a team: 10.TE.AM.2. */
  get ntServer(): string {
    const n = this.#number;
    if (!n) return "—";
    return `10.${Math.floor(n / 100)}.${n % 100}.2`;
  }
}

export const team = new TeamStore();
