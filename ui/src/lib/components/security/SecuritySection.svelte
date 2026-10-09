<script lang="ts">
  // Settings → Security: the one switch (open / secured), and behind it the
  // password and API tokens for tools such as Atlas.
  import { errorText } from "#lib/api/client.js";
  import type { NewApiToken } from "#lib/api/types.js";
  import Icon from "#lib/components/common/Icon.svelte";
  import { timeAgo } from "#lib/format.js";
  import IconButton from "#lib/kit/IconButton.svelte";
  import Prop from "#lib/kit/Prop.svelte";
  import Section from "#lib/kit/Section.svelte";
  import Switch from "#lib/kit/Switch.svelte";
  import { auth, MIN_PASSWORD } from "#lib/stores/auth.svelte.js";
  import { toasts } from "#lib/stores/toasts.svelte.js";
  import SecureForm from "./SecureForm.svelte";

  let pending = $state<"enable" | "disable" | "password" | null>(null);
  let busy = $state(false);
  let error = $state<string | null>(null);
  let current = $state("");
  let next = $state("");
  let again = $state("");
  let label = $state("");
  let created = $state<NewApiToken | null>(null);

  const signedIn = $derived(auth.secured && !!auth.status?.authenticated);
  const switchOn = $derived(auth.secured ? pending !== "disable" : pending === "enable");

  $effect(() => {
    if (signedIn) void auth.loadTokens().catch(() => {});
    // A token shown once must not outlive the session that made it.
    else created = null;
  });

  function reset() {
    pending = null;
    error = null;
    current = next = again = "";
  }

  async function run(action: () => Promise<void>, done: string) {
    busy = true;
    error = null;
    try {
      await action();
      toasts.success(done);
      reset();
    } catch (e) {
      error = errorText(e);
    } finally {
      busy = false;
    }
  }

  function makeOpen(event: SubmitEvent) {
    event.preventDefault();
    void run(() => auth.makeOpen(current), "Security is off. This device is open.");
  }

  function changePassword(event: SubmitEvent) {
    event.preventDefault();
    if (next !== again) {
      error = "The new passwords differ.";
      return;
    }
    void run(() => auth.changePassword(current, next), "Password changed. Other browsers must sign in again.");
  }

  async function createToken(event: SubmitEvent) {
    event.preventDefault();
    if (!label.trim()) return;
    busy = true;
    error = null;
    try {
      created = await auth.createToken(label.trim());
      label = "";
    } catch (e) {
      error = errorText(e);
    } finally {
      busy = false;
    }
  }

  async function revoke(id: string, name: string) {
    try {
      await auth.revokeToken(id);
      toasts.success(`Revoked “${name}”. Tools using it are locked out.`);
    } catch (e) {
      toasts.error(errorText(e));
    }
  }

  async function copy(text: string) {
    try {
      await navigator.clipboard.writeText(text);
      toasts.success("Token copied.");
    } catch {
      toasts.warning("Copy failed; select the token and copy it by hand.");
    }
  }
</script>

<Section title="Security" key="set-security">
  {#if auth.status}
    <div class="state" class:open={auth.open}>
      <Icon name={auth.open ? "lock-open" : "lock"} size={16} />
      {#if auth.open}
        <span><b>Open.</b> Anyone on this network can change this device, install updates and reboot it. Fine on an FRC robot; secure it anywhere else.</span>
      {:else}
        <span><b>Secured.</b> HeliOS needs the device password; tools need an API token.{#if auth.status.via === "token"} You are using an API token.{/if}</span>
      {/if}
    </div>
    {#if auth.status.problem}<p class="err pad">{auth.status.problem}</p>{/if}

    <Prop label="Secure this device" help="Off: no login (FRC default). On: a device password for people, API tokens for tools.">
      <Switch checked={switchOn} label="Secure this device" disabled={busy || (auth.secured && !signedIn)} onchange={(on) => { error = null; pending = on ? (auth.secured ? null : "enable") : auth.secured ? "disable" : null; }} />
    </Prop>

    {#if pending === "enable"}
      <div class="pad"><SecureForm ondone={() => { reset(); toasts.success("This device is secured. Keep the password somewhere safe."); }} oncancel={reset} /></div>
    {:else if pending === "disable"}
      <form class="pad form" onsubmit={makeOpen}>
        <p>Turning security off removes the password and every API token. Anyone on the network can then change this device.</p>
        {#if auth.status.via === "token"}
          <p class="note">Signed in with a token: use the API (<code>POST /v1/auth/disable</code>) or sign in with the password.</p>
        {:else}
          <input type="password" autocomplete="current-password" placeholder="Device password" aria-label="Device password" bind:value={current} disabled={busy} />
          {#if error}<p class="err">{error}</p>{/if}
          <div class="row">
            <button type="submit" class="danger" disabled={busy || !current}>Turn security off</button>
            <button type="button" onclick={reset}>Cancel</button>
          </div>
        {/if}
      </form>
    {/if}

    {#if signedIn}
      <Prop label="Password" help="Changing it signs out every other browser. API tokens keep working.">
        {#if auth.status.password_set_at_ms}<span class="muted">set {timeAgo(auth.status.password_set_at_ms)}</span>{/if}
        <button type="button" class="mini" onclick={() => { reset(); pending = "password"; }}>Change…</button>
        {#if auth.status.via === "session"}
          <button type="button" class="mini" onclick={() => auth.signOut().catch((e) => toasts.error(errorText(e)))}><Icon name="logout" size={11} />Sign out</button>
        {/if}
      </Prop>
      {#if pending === "password"}
        <form class="pad form" onsubmit={changePassword}>
          <input type="password" autocomplete="current-password" placeholder="Current password" aria-label="Current password" bind:value={current} disabled={busy} />
          <input type="password" autocomplete="new-password" placeholder="New password (at least {MIN_PASSWORD} characters)" aria-label="New password" bind:value={next} disabled={busy} />
          <input type="password" autocomplete="new-password" placeholder="New password again" aria-label="Confirm new password" bind:value={again} disabled={busy} />
          {#if error}<p class="err">{error}</p>{/if}
          <div class="row">
            <button type="submit" class="primary" disabled={busy || !current || next.length < MIN_PASSWORD || !again}>Change password</button>
            <button type="button" onclick={reset}>Cancel</button>
          </div>
        </form>
      {/if}

      <div class="pad tokens">
        <div class="thead"><Icon name="key" size={13} /><b>API tokens</b><span class="muted">for Atlas and scripts: <code>Authorization: Bearer &lt;token&gt;</code></span></div>
        {#each auth.tokens as t (t.id)}
          <div class="tok">
            <span class="tl">{t.label}</span>
            <code class="muted">{t.prefix}…</code>
            <span class="muted">made {timeAgo(t.created_at_ms)}{t.last_used_at_ms ? ` · used ${timeAgo(t.last_used_at_ms)}` : " · never used"}</span>
            <IconButton icon="trash" label="Revoke {t.label}" tone="danger" size={22} onclick={() => revoke(t.id, t.label)} />
          </div>
        {:else}
          <p class="muted">No tokens yet.</p>
        {/each}
        {#if created}
          <div class="created">
            <p><b>Copy this token now.</b> It is shown only once; HeliOS keeps only a hash of it.</p>
            <div class="row">
              <code class="secret">{created.token}</code>
              <IconButton icon="copy" label="Copy token" size={24} onclick={() => created && copy(created.token)} />
            </div>
            <button type="button" class="mini" onclick={() => (created = null)}>Done</button>
          </div>
        {/if}
        <form class="row" onsubmit={createToken}>
          <input class="grow" placeholder="Label, e.g. Atlas on the pit laptop" aria-label="Token label" maxlength="64" bind:value={label} disabled={busy} />
          <button type="submit" class="primary" disabled={busy || !label.trim()}>New token</button>
        </form>
        {#if error && pending === null}<p class="err">{error}</p>{/if}
      </div>
    {/if}

    <p class="hint">Lost the password? From a root shell on the device (USB serial console or SSH) run <code>helios-api auth reset</code>: the device becomes open again. HeliOS uses plain HTTP on the robot network; anyone who can watch that network can see the traffic.</p>
  {:else}
    <p class="hint">{auth.error ? `Security status unavailable: ${auth.error}` : "Reading security status…"}</p>
  {/if}
</Section>

<style>
  .state {
    display: flex;
    align-items: flex-start;
    gap: 8px;
    margin: 4px 10px 6px;
    padding: 8px 10px;
    font-size: 12px;
    line-height: 1.5;
    color: var(--fg-2);
    border: 1px solid color-mix(in oklab, var(--ok) 40%, transparent);
    background: color-mix(in oklab, var(--ok) 8%, transparent);
    border-radius: var(--r-2);
  }
  .state :global(svg) {
    flex-shrink: 0;
    margin-top: 1px;
    color: var(--ok);
  }
  .state.open {
    border-color: color-mix(in oklab, var(--warn) 40%, transparent);
    background: color-mix(in oklab, var(--warn) 8%, transparent);
  }
  .state.open :global(svg) {
    color: var(--warn);
  }
  .state b {
    color: var(--fg);
  }
  .pad {
    padding: 4px 10px 8px;
  }
  .form {
    display: flex;
    flex-direction: column;
    gap: 6px;
    max-width: 380px;
  }
  p {
    margin: 0;
    font-size: 11.5px;
    line-height: 1.5;
    color: var(--fg-2);
  }
  .err {
    color: var(--err);
  }
  .note,
  .muted {
    font-size: 11.5px;
    color: var(--fg-3);
  }
  input {
    height: 28px;
    padding: 0 9px;
    font: inherit;
    font-size: 12.5px;
    color: var(--fg);
    background: var(--inset);
    border: 1px solid var(--line-strong);
    border-radius: var(--r-1);
    outline: none;
  }
  input:focus {
    border-color: var(--accent);
  }
  .row {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .grow {
    flex: 1;
    min-width: 0;
  }
  button:not(.mini) {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: 28px;
    padding: 0 12px;
    font-size: 12px;
    font-weight: 600;
    color: var(--fg);
    border: 1px solid var(--line-strong);
    border-radius: var(--r-2);
    white-space: nowrap;
  }
  .primary {
    color: var(--on-accent) !important;
    background: var(--accent);
    border-color: var(--accent) !important;
  }
  .danger {
    color: var(--err) !important;
    border-color: color-mix(in oklab, var(--err) 50%, transparent) !important;
  }
  button:disabled {
    opacity: 0.5;
  }
  .mini {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    height: 22px;
    padding: 0 9px;
    font-size: 11.5px;
    font-weight: 600;
    color: var(--fg-2);
    border: 1px solid var(--line-strong);
    border-radius: var(--r-1);
  }
  .mini:hover {
    color: var(--fg);
    background: var(--s3);
  }
  .tokens {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .thead {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 6px;
    font-size: 12px;
    color: var(--fg);
  }
  .tok {
    display: grid;
    grid-template-columns: minmax(80px, 1fr) auto auto 22px;
    align-items: center;
    gap: 8px;
    min-height: 26px;
    font-size: 12px;
  }
  .tl {
    font-weight: 600;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .created {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 6px;
    padding: 8px;
    border: 1px solid var(--accent-ring);
    background: var(--accent-tint);
    border-radius: var(--r-2);
  }
  .secret {
    padding: 4px 6px;
    font-size: 11px;
    word-break: break-all;
    user-select: all;
    color: var(--fg);
    background: var(--inset);
    border-radius: var(--r-1);
  }
  code {
    font-family: var(--font-code);
    font-size: 11px;
  }
  .hint {
    margin: 4px 10px 8px;
    font-size: 11.5px;
    color: var(--fg-3);
    line-height: 1.5;
  }
</style>
