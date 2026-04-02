<script lang="ts">
  import { resolve } from '$app/paths';
  import { page } from '$app/stores';
  import { faTriangleExclamation } from '@fortawesome/free-solid-svg-icons';
  import FaIcon from '$lib/components/icons/FaIcon.svelte';

  import {
    dismissOsHealthBanner,
    dismissedOsHealthFingerprint,
    osHealthBanner,
    osHealthFingerprint,
    resourceGuardBanner
  } from './appShellState';

  const isSettingsPage = $derived($page.url.pathname === '/settings' || $page.url.pathname.startsWith('/settings/'));
  const showOsHealthBanner = $derived(
    Boolean($osHealthBanner) && !isSettingsPage && $dismissedOsHealthFingerprint !== $osHealthFingerprint
  );
</script>

{#if showOsHealthBanner && $osHealthBanner}
  <div class="mb-3 rounded border border-error-500/40 bg-error-500/10 px-4 py-3 text-error-50 shadow-[0_0_0_1px_rgba(239,68,68,0.12)]" role="alert">
    <div class="flex items-start gap-3">
      <FaIcon icon={faTriangleExclamation} class="mt-0.5 h-4 w-4 shrink-0 text-error-200" />
      <div class="min-w-0 flex-1">
        <div class="text-sm font-semibold leading-tight">{$osHealthBanner.title}</div>
        <div class="mt-1 text-xs leading-relaxed text-error-100">{$osHealthBanner.details}</div>
        <div class="mt-3 flex flex-wrap items-center gap-2">
          <a class="btn btn-xs variant-soft" href={`${resolve('/settings')}?tab=diagnostics`}>Open diagnostics</a>
          <button class="btn btn-xs btn-outline" type="button" onclick={dismissOsHealthBanner}>Dismiss</button>
        </div>
      </div>
    </div>
  </div>
{/if}

{#if $resourceGuardBanner}
  <div class="mb-3 rounded border border-warning-500/40 bg-warning-500/10 px-4 py-3 text-warning-50 shadow-[0_0_0_1px_rgba(250,204,21,0.12)]" role="alert">
    <div class="flex items-start gap-3">
      <FaIcon icon={faTriangleExclamation} class="mt-0.5 h-4 w-4 shrink-0 text-warning-200" />
      <div class="min-w-0">
        <div class="text-sm font-semibold leading-tight">{$resourceGuardBanner.title}</div>
        <div class="mt-1 text-xs leading-relaxed text-warning-100">{$resourceGuardBanner.details}</div>
      </div>
    </div>
  </div>
{/if}
