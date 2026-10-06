<script lang="ts">
  // Pick a device password (twice) and secure the device. Used by Settings and
  // by first-run setup.
  import { errorText } from "$lib/api/client";
  import Icon from "$lib/components/common/Icon.svelte";
  import { auth, MIN_PASSWORD } from "$lib/stores/auth.svelte";

  let { ondone, oncancel }: { ondone?: () => void; oncancel?: () => void } = $props();

  let password = $state("");
  let confirm = $state("");
  let busy = $state(false);
  let error = $state<string | null>(null);
  const problem = $derived(
    password.length === 0 ? null : password.length < MIN_PASSWORD ? `At least ${MIN_PASSWORD} characters.` : confirm && confirm !== password ? "The passwords differ." : null,
  );
  const canSubmit = $derived(password.length >= MIN_PASSWORD && confirm === password && !busy);

  async function submit(event: SubmitEvent) {
    event.preventDefault();
    if (!canSubmit) return;
    busy = true;
    error = null;
    try {
      await auth.secure(password);
      password = confirm = "";
      ondone?.();
    } catch (e) {
      error = errorText(e);
    } finally {
      busy = false;
    }
  }
</script>

<form class="secure" onsubmit={submit}>
  <input type="password" autocomplete="new-password" placeholder="Device password" aria-label="Device password" bind:value={password} disabled={busy} />
  <input type="password" autocomplete="new-password" placeholder="Same password again" aria-label="Confirm device password" bind:value={confirm} disabled={busy} />
  {#if problem || error}<p class="err">{error ?? problem}</p>{/if}
  <p class="note">Everyone who opens HeliOS on this device will need it. Tools like Atlas use API tokens, made in Settings. If it is lost, <code>heliosctl auth reset</code> from a root shell on the device opens it again.</p>
  <div class="row">
    <button type="submit" class="primary" disabled={!canSubmit}><Icon name="lock" size={13} />{busy ? "Securing…" : "Secure this device"}</button>
    {#if oncancel}<button type="button" onclick={oncancel}>Cancel</button>{/if}
  </div>
</form>

<style>
  .secure {
    display: flex;
    flex-direction: column;
    gap: 6px;
    max-width: 360px;
  }
  input {
    height: 30px;
    padding: 0 9px;
    font: inherit;
    font-size: 13px;
    color: var(--fg);
    background: var(--inset);
    border: 1px solid var(--line-strong);
    border-radius: var(--r-1);
    outline: none;
  }
  input:focus {
    border-color: var(--accent);
  }
  p {
    margin: 0;
    font-size: 11.5px;
    line-height: 1.5;
  }
  .err {
    color: var(--err);
  }
  .note {
    color: var(--fg-3);
  }
  code {
    font-family: var(--font-code);
    color: var(--fg-2);
  }
  .row {
    display: flex;
    gap: 6px;
  }
  button {
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
  }
  .primary {
    color: var(--on-accent);
    background: var(--accent);
    border-color: var(--accent);
  }
  button:disabled {
    opacity: 0.5;
  }
</style>
