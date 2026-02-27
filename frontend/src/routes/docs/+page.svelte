<svelte:head>
  <title>Docs</title>
</svelte:head>

<script lang="ts">
  import { onDestroy } from 'svelte';

  let docsFrame = $state<HTMLIFrameElement | null>(null);
  let docsCleanup: (() => void) | null = null;

  const isDev = import.meta.env.DEV;
  const docsBasePath = '/docs/';
  const docsSrc = isDev ? '/docs/index.html' : docsBasePath;

  function normalizeDocsHref(href: string, base?: string): string | null {
    try {
      if (href.startsWith('#')) return null;
      const url = new URL(href, base ?? window.location.origin);
      if (url.origin !== window.location.origin) return null;
      if (!url.pathname.startsWith(docsBasePath)) return null;

      const hasExtension = /\.[a-zA-Z0-9]+$/.test(url.pathname);
      let pathname = url.pathname;

      if (pathname === '/docs') {
        pathname = docsBasePath;
      }

      if (!pathname.endsWith('/') && !hasExtension) {
        pathname = `${pathname}/`;
      }

      if (isDev && pathname.endsWith('/') && !hasExtension) {
        pathname = `${pathname}index.html`;
      }

      url.pathname = pathname;
      return `${url.pathname}${url.search}${url.hash}`;
    } catch {
      return null;
    }
  }

  function handleDocsLoad(): void {
    docsCleanup?.();
    const frame = docsFrame;
    const frameDoc = frame?.contentWindow?.document ?? null;
    if (!frame || !frameDoc) return;
    const clickHandler = (event: MouseEvent) => {
      const target = event.target as Element | null;
      const link = target?.closest?.('a[href]') as HTMLAnchorElement | null;
      if (!link) return;
      const baseHref = frame.contentWindow?.location.href ?? window.location.origin;
      const normalized = normalizeDocsHref(link.getAttribute('href') ?? link.href, baseHref);
      if (!normalized) return;
      event.preventDefault();
      frame.contentWindow?.location.assign(normalized);
    };
    frameDoc.addEventListener('click', clickHandler, true);
    docsCleanup = () => frameDoc.removeEventListener('click', clickHandler, true);
  }

  onDestroy(() => {
    docsCleanup?.();
  });
</script>

<div class="flex min-h-0 flex-1">
  <iframe
    title="Helios documentation"
    src={docsSrc}
    class="h-full w-full flex-1 border-0"
    loading="lazy"
    sandbox="allow-same-origin allow-scripts allow-forms allow-popups"
    bind:this={docsFrame}
    onload={handleDocsLoad}
  ></iframe>
</div>
