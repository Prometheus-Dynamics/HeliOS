<script lang="ts">
  // Tooltips for anything with `data-tip`: one element, positioned on hover
  // after a short delay, instant while moving between tipped controls.
  let tip = $state<{ text: string; x: number; y: number; below: boolean } | null>(null);
  let timer = 0;
  let warm = false;
  let coolTimer = 0;

  function show(el: HTMLElement) {
    const text = el.dataset.tip;
    if (!text) return;
    const r = el.getBoundingClientRect();
    const below = r.top < 40;
    tip = { text, x: Math.min(window.innerWidth - 8, Math.max(8, r.left + r.width / 2)), y: below ? r.bottom + 6 : r.top - 6, below };
    warm = true;
  }

  function over(event: PointerEvent) {
    const el = (event.target as HTMLElement).closest<HTMLElement>("[data-tip]");
    clearTimeout(timer);
    if (!el) {
      tip = null;
      clearTimeout(coolTimer);
      coolTimer = window.setTimeout(() => (warm = false), 300);
      return;
    }
    clearTimeout(coolTimer);
    if (warm) show(el);
    else timer = window.setTimeout(() => show(el), 450);
  }
</script>

<svelte:window onpointerover={over} onpointerdown={() => { clearTimeout(timer); tip = null; }} onwheel={() => (tip = null)} />

{#if tip}
  <div class="tip" class:below={tip.below} style:left="{tip.x}px" style:top="{tip.y}px" role="tooltip">{tip.text}</div>
{/if}

<style>
  .tip {
    position: fixed;
    z-index: 1100;
    transform: translate(-50%, -100%);
    max-width: 280px;
    padding: 4px 8px;
    font-size: 11.5px;
    line-height: 1.35;
    color: var(--fg);
    background: var(--s3);
    border: 1px solid var(--line-strong);
    border-radius: var(--r-1);
    box-shadow: var(--shadow);
    pointer-events: none;
    white-space: pre-line;
  }
  .tip.below {
    transform: translate(-50%, 0);
  }
</style>
