<script lang="ts">
  import { resolve } from '$app/paths';

  type SectionRoute = '/dashboard' | '/pipelines' | '/devices' | '/media' | '/systems' | '/docs';

  const sections: Array<{ href: SectionRoute; title: string; description: string }> = [
    {
      href: '/dashboard',
      title: 'Dashboard',
      description: 'High-level health summary, stream counts, and quick actions.'
    },
    {
      href: '/pipelines',
      title: 'Pipelines',
      description: 'Visual pipeline builder, graph overview, and resource monitoring.'
    },
    {
      href: '/devices',
      title: 'Devices',
      description: 'Cameras, IMUs, and registration workflows focused on isolated components.'
    },
    {
      href: '/media',
      title: 'Media',
      description: 'Upload image/video playlists that power the file-based capture backends.'
    },
    {
      href: '/systems',
      title: 'Systems',
      description: 'Peers, services, logs, diagnostics, and deployment knobs.'
    },
    {
      href: '/docs',
      title: 'Docs',
      description: 'Guides, HTTP + WS specs, and future SDK references.'
    }
  ];
</script>

<section class="max-w-4xl space-y-6">
  <header class="space-y-2">
    <p class="text-xs uppercase tracking-widest text-surface-500">Helios Frontend</p>
    <h1 class="text-4xl font-bold">Fresh start.</h1>
    <p class="text-lg text-surface-500">
      The previous dashboard has been removed. We are rebuilding the interface from isolated, composable
      components. For now, the shell is all that remains.
    </p>
  </header>

  <div class="space-y-2 rounded border border-dashed border-surface-300 p-4">
    <p class="text-sm uppercase tracking-wide text-surface-500">Navigation Preview</p>
    <ul class="grid gap-3 md:grid-cols-2">
      {#each sections as section (section.href)}
        <li class="rounded border border-surface-800 bg-surface-900/40 p-4 shadow-inner">
          <div class="flex items-center justify-between gap-3">
            <div>
              <p class="text-xs uppercase tracking-wide text-surface-500">{section.href}</p>
              <h2 class="text-xl font-semibold">{section.title}</h2>
            </div>
            <a class="btn preset-tonal" href={resolve(section.href)}>Open</a>
          </div>
          <p class="mt-2 text-sm text-surface-400">{section.description}</p>
        </li>
      {/each}
    </ul>
  </div>
</section>
