<script lang="ts">
  import { resolve } from '$app/paths';
  import { page } from '$app/stores';
  import type { IconDefinition } from '@fortawesome/free-solid-svg-icons';
  import {
    faAnglesLeft,
    faBookOpen,
    faCamera,
    faDiagramProject,
    faGaugeHigh,
    faGear,
    faGlobe,
    faImages,
    faMicrochip,
    faTowerBroadcast
  } from '@fortawesome/free-solid-svg-icons';
  import ConnectionStatusBanner from '$lib/components/ConnectionStatusBanner.svelte';
  import FaIcon from '$lib/components/icons/FaIcon.svelte';

  import {
    showSettingsBootloaderWarning,
    showSettingsOsWarning,
    sidebarCollapsed,
    toggleSidebar
  } from './appShellState';

  type NavHref =
    | '/dashboard'
    | '/pipelines'
    | '/devices'
    | '/peers'
    | '/media'
    | '/localization'
    | '/systems'
    | '/docs'
    | '/settings';

  const navSections: Array<{ href: Exclude<NavHref, '/settings'>; label: string; hint: string; icon: IconDefinition }> = [
    { href: '/dashboard', label: 'Dashboard', hint: 'Overview & health', icon: faGaugeHigh },
    { href: '/pipelines', label: 'Pipelines', hint: 'Graphs & IO', icon: faDiagramProject },
    { href: '/devices', label: 'Devices', hint: 'Cameras & sensors', icon: faCamera },
    { href: '/peers', label: 'Peers', hint: 'Cluster management', icon: faTowerBroadcast },
    { href: '/media', label: 'Media', hint: 'File management', icon: faImages },
    { href: '/localization', label: 'Localization', hint: '3D mapping', icon: faGlobe },
    { href: '/systems', label: 'Systems', hint: 'Runtime internals', icon: faMicrochip },
    { href: '/docs', label: 'Docs', hint: 'Guides & APIs', icon: faBookOpen }
  ];

  const settingsLink: { href: '/settings'; label: string; icon: IconDefinition } = {
    href: '/settings',
    label: 'Settings',
    icon: faGear
  };

  const isActive = (href: string): boolean => {
    const current = $page.url.pathname;
    return current === href || current.startsWith(`${href}/`);
  };
</script>

<aside
  class={`app-sidebar ${$sidebarCollapsed ? 'app-sidebar--collapsed px-2' : 'px-4'} flex w-full shrink-0 flex-col border border-surface-800 bg-surface-900/80 py-4 transition-[width,padding] duration-200 lg:sticky lg:py-4`}
>
  <div class={`flex items-center ${$sidebarCollapsed ? 'justify-center pb-4' : 'justify-between pb-3'}`}>
    {#if !$sidebarCollapsed}
      <div class="text-xs font-semibold uppercase tracking-[0.3em] text-surface-500">
        Helios
      </div>
    {/if}
    <button
      type="button"
      class={`inline-flex h-8 w-8 items-center justify-center rounded-md border border-surface-700 text-surface-400 transition hover:border-surface-500 hover:text-surface-50 ${$sidebarCollapsed ? '' : 'ml-auto'}`}
      onclick={toggleSidebar}
      aria-label={$sidebarCollapsed ? 'Expand sidebar' : 'Collapse sidebar'}
      title={$sidebarCollapsed ? 'Expand sidebar' : 'Collapse sidebar'}
    >
      <FaIcon icon={faAnglesLeft} class={`h-3.5 w-3.5 transition-transform ${$sidebarCollapsed ? 'rotate-180' : ''}`} />
    </button>
  </div>

  <nav class={`space-y-2 border-t border-surface-800/80 pt-4 ${$sidebarCollapsed ? 'flex flex-col items-center' : ''}`}>
    {#each navSections as section (section.href)}
      <a
        class={`block ${
          $sidebarCollapsed ? 'h-11 w-11 rounded-lg border' : 'border-l-2'
        } px-3 py-1.5 text-xs transition 2xl:py-2 ${
          isActive(section.href)
            ? $sidebarCollapsed
              ? 'border-primary-500/70 bg-surface-900 text-primary-200'
              : 'border-primary-500 bg-surface-900 text-primary-200'
            : $sidebarCollapsed
              ? 'border-transparent text-surface-400 hover:border-surface-700 hover:bg-surface-900/70 hover:text-surface-50'
              : 'border-transparent text-surface-400 hover:border-surface-500 hover:text-surface-50'
        }`}
        href={resolve(section.href)}
        aria-label={section.label}
        title={$sidebarCollapsed ? section.label : undefined}
      >
        <div class={`flex ${$sidebarCollapsed ? 'h-full items-center justify-center' : 'min-w-0 items-center gap-2.5'} leading-tight`}>
          <FaIcon icon={section.icon} class="h-4 w-4 shrink-0" />
          {#if $sidebarCollapsed}
            <span class="sr-only">{section.label}</span>
          {:else}
            <div class="min-w-0 flex flex-col leading-tight">
              <span class="helios-nav-label truncate font-semibold">{section.label}</span>
              <span class="helios-nav-hint truncate uppercase opacity-60">{section.hint}</span>
            </div>
          {/if}
        </div>
      </a>
    {/each}
  </nav>

  <div class="mt-auto space-y-3">
    <ConnectionStatusBanner compact={$sidebarCollapsed} />
    <div class={`border-t border-surface-800/80 pt-4 ${$sidebarCollapsed ? 'flex justify-center' : ''}`}>
      <a
        class={`relative flex min-w-0 items-center ${
          $sidebarCollapsed ? 'h-11 w-11 justify-center rounded-lg border border-transparent' : 'gap-2 px-3'
        } py-1.5 text-sm uppercase tracking-[0.3em] transition 2xl:py-2 ${
          isActive(settingsLink.href)
            ? $sidebarCollapsed
              ? 'border-primary-500/70 bg-surface-900 text-primary-200'
              : 'text-primary-200'
            : $sidebarCollapsed
              ? 'text-surface-500 hover:border-surface-700 hover:bg-surface-900/70 hover:text-surface-50'
              : 'text-surface-500 hover:text-surface-50'
        }`}
        href={resolve(settingsLink.href)}
        aria-label={settingsLink.label}
        title={$sidebarCollapsed ? settingsLink.label : undefined}
      >
        <FaIcon icon={settingsLink.icon} class="h-4 w-4 shrink-0" />
        {#if !$sidebarCollapsed}
          <span class="helios-settings-label truncate font-semibold">{settingsLink.label}</span>
        {:else}
          <span class="sr-only">{settingsLink.label}</span>
        {/if}
        {#if $showSettingsBootloaderWarning || $showSettingsOsWarning}
          <span class="sr-only">{$showSettingsOsWarning ? 'Core OS issue detected' : 'Bootloader update required'}</span>
          <span
            class={`helios-nav-warning ${$sidebarCollapsed ? 'absolute -right-0.5 -top-0.5' : 'ml-auto'}`}
            title={$showSettingsOsWarning ? 'Core OS issue detected' : 'Bootloader update required'}
            aria-hidden="true"
          >
            <svg viewBox="0 0 24 24" class="h-4 w-4" focusable="false">
              <path d="M12 2 1 21h22L12 2Z" fill="currentColor" />
              <path d="M12 8v6" stroke="#0b0f1a" stroke-width="2" stroke-linecap="round" />
              <circle cx="12" cy="17" r="1.2" fill="#0b0f1a" />
            </svg>
          </span>
        {/if}
      </a>
    </div>
  </div>
</aside>

<style>
  @media (min-width: 1024px) {
    .app-sidebar {
      top: var(--app-shell-pad);
      width: var(--app-sidebar-width);
    }

    .app-sidebar--collapsed {
      width: var(--app-sidebar-collapsed-width);
    }
  }

  .helios-nav-label {
    font-size: 0.8rem;
    line-height: 1.2;
    letter-spacing: 0.015em;
  }

  .helios-nav-hint {
    font-size: 0.6rem;
    line-height: 1.15;
    letter-spacing: 0.22em;
  }

  .helios-settings-label {
    font-size: 0.68rem;
    line-height: 1.15;
    letter-spacing: 0.24em;
  }

  @media (max-height: 980px), (max-width: 1400px) {
    .helios-nav-label {
      font-size: 0.74rem;
    }

    .helios-nav-hint {
      font-size: 0.56rem;
      letter-spacing: 0.2em;
    }

    .helios-settings-label {
      font-size: 0.62rem;
      letter-spacing: 0.2em;
    }

    .app-sidebar:not(.app-sidebar--collapsed) nav a {
      padding-top: 0.3rem;
      padding-bottom: 0.3rem;
    }
  }

  @media (max-height: 860px), (max-width: 1220px) {
    .helios-nav-label {
      font-size: 0.7rem;
    }

    .helios-nav-hint {
      font-size: 0.52rem;
      letter-spacing: 0.17em;
    }

    .helios-settings-label {
      font-size: 0.58rem;
      letter-spacing: 0.17em;
    }
  }

  .helios-nav-warning {
    color: rgba(250, 204, 21, 0.95);
    filter:
      drop-shadow(0 0 6px rgba(250, 204, 21, 0.55))
      drop-shadow(0 0 14px rgba(250, 204, 21, 0.35));
    animation: helios-nav-warning-pulse 1.4s ease-in-out infinite;
  }

  @keyframes helios-nav-warning-pulse {
    0%,
    100% {
      transform: scale(1);
      opacity: 0.95;
      filter:
        drop-shadow(0 0 6px rgba(250, 204, 21, 0.55))
        drop-shadow(0 0 14px rgba(250, 204, 21, 0.35));
    }

    50% {
      transform: scale(1.06);
      opacity: 1;
      filter:
        drop-shadow(0 0 10px rgba(250, 204, 21, 0.7))
        drop-shadow(0 0 22px rgba(250, 204, 21, 0.45));
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .helios-nav-warning {
      animation: none;
    }
  }
</style>
