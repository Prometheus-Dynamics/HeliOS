<script lang="ts">
  // Shown instead of everything else when the device is secured and this
  // browser is not signed in. One field: the device password.
  import { errorText } from "$lib/api/client";
  import Icon from "$lib/components/common/Icon.svelte";
  import { auth } from "$lib/stores/auth.svelte";

  let password = $state("");
  let busy = $state(false);
  let error = $state<string | null>(null);
  let input = $state<HTMLInputElement>();
  const host = typeof window === "undefined" ? "this device" : window.location.host;

  $effect(() => {
    input?.focus();
  });

  async function submit(event: SubmitEvent) {
    event.preventDefault();
    if (!password || busy) return;
    busy = true;
    error = null;
    try {
      await auth.signIn(password);
      password = "";
    } catch (e) {
      error = errorText(e);
    } finally {
      busy = false;
    }
  }
</script>

<div class="scrim"></div>
<form class="dlg" aria-label="Sign in to this device" onsubmit={submit}>
  <span class="badge"><Icon name="lock" size={20} /></span>
  <h2>This device is secured</h2>
  <p>Enter the device password to use HeliOS on <b>{host}</b>.</p>
  {#if auth.status?.problem}
    <p class="err">{auth.status.problem}</p>
  {/if}
  <input bind:this={input} type="password" autocomplete="current-password" placeholder="Device password" aria-label="Device password" bind:value={password} disabled={busy} />
  {#if error}<p class="err">{error}</p>{/if}
  <button type="submit" class="primary" disabled={busy || !password}>{busy ? "Signing in…" : "Sign in"}</button>
  <details>
    <summary>Lost the password?</summary>
    <p>Open a root shell on the device (the USB serial console, or SSH) and run <code>helios-api auth reset</code>. The device becomes open again, with no password and no API tokens; secure it again from Settings.</p>
  </details>
</form>

<style>
  .scrim {
    position: fixed;
    inset: 0;
    z-index: 980;
    background: var(--s0);
  }
  .dlg {
    position: fixed;
    z-index: 981;
    top: 50%;
    left: 50%;
    transform: translate(-50%, -50%);
    width: min(380px, calc(100vw - 32px));
    display: flex;
    flex-direction: column;
    gap: 10px;
    padding: 22px;
    background: var(--s1);
    border: 1px solid var(--line-strong);
    border-radius: var(--r-3);
    box-shadow: var(--shadow);
    color: var(--fg);
  }
  .badge {
    display: grid;
    place-items: center;
    width: 38px;
    height: 38px;
    border-radius: var(--r-2);
    color: var(--ok);
    background: color-mix(in oklab, var(--ok) 16%, transparent);
  }
  h2 {
    margin: 0;
    font-size: 17px;
  }
  p {
    margin: 0;
    font-size: 12.5px;
    line-height: 1.5;
    color: var(--fg-2);
  }
  .err {
    color: var(--err);
  }
  input {
    height: 34px;
    padding: 0 10px;
    font: inherit;
    font-size: 14px;
    color: var(--fg);
    background: var(--inset);
    border: 1px solid var(--line-strong);
    border-radius: var(--r-2);
    outline: none;
  }
  input:focus {
    border-color: var(--accent);
  }
  .primary {
    height: 32px;
    font-size: 13px;
    font-weight: 600;
    color: var(--on-accent);
    background: var(--accent);
    border-radius: var(--r-2);
  }
  .primary:disabled {
    opacity: 0.5;
  }
  details {
    font-size: 12px;
    color: var(--fg-3);
  }
  summary {
    cursor: pointer;
  }
  details p {
    margin-top: 6px;
    font-size: 12px;
  }
  code {
    font-family: var(--font-code);
    color: var(--fg);
  }
</style>
