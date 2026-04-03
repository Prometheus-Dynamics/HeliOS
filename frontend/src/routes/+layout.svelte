<script lang="ts">
  import '../app.css';
  import '$lib/api/client';
  import { page } from '$app/stores';
  import { Toast } from '@skeletonlabs/skeleton-svelte';
  import type { Snippet } from 'svelte';
  import FloatingPipelineOutputsViewer from '$lib/components/FloatingPipelineOutputsViewer.svelte';
  import FloatingStreamViewer from '$lib/components/FloatingStreamViewer.svelte';
  import NotificationCenter from '$lib/components/NotificationCenter.svelte';
  import { toaster } from '$lib/toaster';

  import AppShellBanners from './AppShellBanners.svelte';
  import AppShellRuntime from './AppShellRuntime.svelte';
  import AppShellSidebar from './AppShellSidebar.svelte';

  let { children }: { children: Snippet } = $props();

  const isConsolePopout = $derived($page.url.pathname.startsWith('/console/'));
  const isDocsPage = $derived($page.url.pathname === '/docs' || $page.url.pathname.startsWith('/docs/'));
  const isMediaPage = $derived($page.url.pathname === '/media' || $page.url.pathname.startsWith('/media/'));
  const isPipelinesPage = $derived($page.url.pathname === '/pipelines' || $page.url.pathname.startsWith('/pipelines/'));
  const isLocalizationPage = $derived($page.url.pathname === '/localization' || $page.url.pathname.startsWith('/localization/'));
  const isSettingsPage = $derived($page.url.pathname === '/settings' || $page.url.pathname.startsWith('/settings/'));
  const isSystemsPage = $derived($page.url.pathname === '/systems' || $page.url.pathname.startsWith('/systems/'));
  const isViewportLockedPage = $derived(
    isMediaPage || isPipelinesPage || isLocalizationPage || isSettingsPage || isSystemsPage
  );
</script>

<svelte:head>
  <title>Helios</title>
</svelte:head>

<Toast.Group toaster={toaster} class="pointer-events-none fixed inset-x-0 top-4 z-[110] mx-auto flex max-w-lg flex-col gap-3 px-4">
  {#snippet children(toast)}
    <Toast
      {toast}
      class={`pointer-events-auto border bg-surface-950/95 shadow-lg backdrop-blur ${
        toast.type === 'error'
          ? 'border-error-400/60 text-error-50'
          : toast.type === 'success'
            ? 'border-success-400/60 text-success-50'
            : toast.type === 'warning'
              ? 'border-warning-400/60 text-warning-50'
              : 'border-surface-700/70 text-surface-50'
      }`}
    >
      <div class="flex items-start gap-3 p-4">
        <div class="min-w-0 flex-1 space-y-1">
          {#if toast.title}
            <Toast.Title class="text-sm font-semibold">{String(toast.title)}</Toast.Title>
          {/if}
          {#if toast.description}
            <Toast.Description class="text-sm text-surface-300">
              {String(toast.description)}
            </Toast.Description>
          {/if}
        </div>
        {#if toast.action}
          <Toast.ActionTrigger class="btn btn-xs variant-soft shrink-0">
            {toast.action.label}
          </Toast.ActionTrigger>
        {/if}
        {#if toast.closable !== false}
          <Toast.CloseTrigger class="shrink-0 text-xs uppercase tracking-[0.18em] text-surface-400 transition hover:text-surface-50">
            Dismiss
          </Toast.CloseTrigger>
        {/if}
      </div>
    </Toast>
  {/snippet}
</Toast.Group>
<AppShellRuntime />

<div
  class={`${isViewportLockedPage ? 'h-screen h-[100svh] h-[100dvh] overflow-hidden' : 'min-h-screen min-h-[100svh] min-h-[100dvh]'} bg-surface-950 text-surface-50`}
  data-theme="helios"
>
  {#if isConsolePopout}
    <main class="flex min-h-screen min-h-[100svh] min-h-[100dvh] flex-col overflow-hidden p-4">
      <div class="flex min-h-0 flex-1 flex-col overflow-auto border border-surface-800 bg-surface-900/40 p-4">
        {@render children()}
      </div>
    </main>
  {:else}
    <div
      class={`app-shell flex ${isViewportLockedPage ? 'h-full overflow-hidden' : 'min-h-screen min-h-[100svh] min-h-[100dvh]'} flex-col lg:flex-row`}
    >
      <AppShellSidebar />
      <main
        class={`app-main flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden border border-surface-800 bg-surface-900/30 ${
          isDocsPage ? 'app-main--docs' : 'px-4 py-4'
        }`}
      >
        <AppShellBanners />
        <div class={`flex min-h-0 flex-1 flex-col ${isViewportLockedPage ? 'overflow-hidden' : 'overflow-auto'}`}>
          {@render children()}
        </div>
      </main>
    </div>
    <FloatingStreamViewer />
    <FloatingPipelineOutputsViewer />
    <NotificationCenter />
  {/if}
</div>

<style>
  .app-shell {
    gap: var(--app-shell-gap);
    padding: var(--app-shell-pad);
  }

  @media (min-width: 1024px) {
    .app-main {
      padding-inline: var(--app-main-pad-x);
      padding-block: var(--app-main-pad-y);
    }

    .app-main--docs {
      padding: 0;
    }
  }
</style>
