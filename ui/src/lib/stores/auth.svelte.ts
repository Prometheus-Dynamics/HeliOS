// Device security: is this device open or secured, and is this browser signed
// in? Open is the default (FRC robots run open, no login). Securing it is one
// password; tools use API tokens. The top bar always shows which it is.
// With `?mock=1` the same flow runs against an in-memory device.

import { ApiError, api, errorText, session } from "#lib/api/client.js";
import { MOCK } from "#lib/api/mode.js";
import type { ApiToken, AuthStatus, NewApiToken } from "#lib/api/types.js";

export const MIN_PASSWORD = 8;

/** The mock device: open until secured, one password, tokens in memory. */
class MockDevice {
  password: string | null = null;
  signedIn = false;
  tokens: ApiToken[] = [];

  status(): AuthStatus {
    if (this.password === null) return { mode: "open", authenticated: true, via: "open" };
    if (!this.signedIn) return { mode: "secured", authenticated: false, via: null };
    return { mode: "secured", authenticated: true, via: "session", csrf_token: "mock", tokens: this.tokens.length, password_set_at_ms: Date.now() };
  }

  check(password: string) {
    if (password !== this.password) throw new ApiError(401, "unauthorized", "wrong password");
  }
}

class AuthStore {
  status = $state<AuthStatus | null>(null);
  /** Set when the status could not be read (device unreachable). */
  error = $state<string | null>(null);
  tokens = $state<ApiToken[]>([]);
  private mock = new MockDevice();

  constructor() {
    session.onUnauthorized(() => void this.refresh());
  }

  get secured(): boolean {
    return this.status?.mode === "secured";
  }

  get open(): boolean {
    return this.status?.mode === "open";
  }

  /** Secured and this browser is not signed in: show the sign-in screen. */
  get needsSignIn(): boolean {
    return !!this.status && this.status.mode === "secured" && !this.status.authenticated;
  }

  /** The rest of the UI may load data. */
  get ready(): boolean {
    return !!this.status?.authenticated || (!this.status && !!this.error);
  }

  private apply(status: AuthStatus) {
    this.status = status;
    this.error = null;
    session.setCsrf(status.csrf_token);
  }

  async refresh(): Promise<void> {
    if (MOCK) return this.apply(this.mock.status());
    try {
      this.apply(await api.authStatus());
    } catch (error) {
      this.error = errorText(error);
    }
  }

  async signIn(password: string): Promise<void> {
    if (MOCK) {
      this.mock.check(password);
      this.mock.signedIn = true;
      return this.apply(this.mock.status());
    }
    this.apply(await api.login(password));
  }

  async signOut(): Promise<void> {
    if (MOCK) {
      this.mock.signedIn = false;
      return this.apply(this.mock.status());
    }
    await api.logout();
    session.setCsrf(null);
    await this.refresh();
  }

  /** Open → secured. This browser stays signed in. */
  async secure(password: string): Promise<void> {
    if (password.length < MIN_PASSWORD) throw new Error(`Use at least ${MIN_PASSWORD} characters.`);
    if (MOCK) {
      this.mock.password = password;
      this.mock.signedIn = true;
      return this.apply(this.mock.status());
    }
    this.apply(await api.enableAuth(password));
  }

  /** Secured → open. Needs the password again. */
  async makeOpen(password: string): Promise<void> {
    if (MOCK) {
      this.mock.check(password);
      this.mock = new MockDevice();
      this.tokens = [];
      return this.apply(this.mock.status());
    }
    this.apply(await api.disableAuth(password));
    this.tokens = [];
  }

  async changePassword(current: string, next: string): Promise<void> {
    if (next.length < MIN_PASSWORD) throw new Error(`Use at least ${MIN_PASSWORD} characters.`);
    if (MOCK) {
      this.mock.check(current);
      this.mock.password = next;
      return this.apply(this.mock.status());
    }
    this.apply(await api.changePassword(current, next));
  }

  async loadTokens(): Promise<void> {
    if (MOCK) {
      this.tokens = [...this.mock.tokens];
      return;
    }
    this.tokens = this.secured && this.status?.authenticated ? await api.tokens() : [];
  }

  async createToken(label: string): Promise<NewApiToken> {
    let created: NewApiToken;
    if (MOCK) {
      const hex = Array.from(crypto.getRandomValues(new Uint8Array(32)), (b) => b.toString(16).padStart(2, "0")).join("");
      created = { id: `tok_${hex.slice(0, 12)}`, label, prefix: `helios_${hex.slice(0, 8)}`, created_at_ms: Date.now(), last_used_at_ms: null, token: `helios_${hex}` };
      const { token: _secret, ...listed } = created;
      this.mock.tokens.push(listed);
    } else {
      created = await api.createToken(label);
    }
    await this.loadTokens();
    return created;
  }

  async revokeToken(id: string): Promise<void> {
    if (MOCK) this.mock.tokens = this.mock.tokens.filter((t) => t.id !== id);
    else await api.revokeToken(id);
    await this.loadTokens();
  }
}

export const auth = new AuthStore();
