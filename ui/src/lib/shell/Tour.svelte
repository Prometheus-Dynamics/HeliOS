<script lang="ts">
  // The guided tour: dims everything except the element being explained and
  // puts a short note beside it.
  import { help, TOUR } from "./help.svelte";

  let rect = $state<DOMRect | null>(null);
  let card = $state<HTMLDivElement>();
  let pos = $state({ left: 0, top: 0 });
  const step = $derived(help.tourStep === null ? null : TOUR[help.tourStep]);

  $effect(() => {
    if (!step) return;
    const find = () => {
      const el = document.querySelector(step.target);
      rect = el ? el.getBoundingClientRect() : null;
      if (!card || !rect) return;
      const w = card.offsetWidth;
      const h = card.offsetHeight;
      // Prefer the right side, then below, then above.
      let left = rect.right + 14;
      let top = rect.top;
      if (left + w > innerWidth - 8) {
        left = Math.min(innerWidth - w - 8, Math.max(8, rect.left));
        top = rect.bottom + 12;
        if (top + h > innerHeight - 8) top = rect.top - h - 12;
      }
      pos = { left, top: Math.max(8, Math.min(innerHeight - h - 8, top)) };
    };
    find();
    const t = setInterval(find, 300);
    return () => clearInterval(t);
  });
</script>

<svelte:window onkeydown={(e) => {
  if (!step) return;
  if (e.key === "Escape") help.endTour();
  if (e.key === "ArrowRight" || e.key === "Enter") help.next();
  if (e.key === "ArrowLeft") help.back();
}} />

{#if step}
  <div class="veil" role="presentation">
    {#if rect}
      <div class="hole" style:left="{rect.left - 4}px" style:top="{rect.top - 4}px" style:width="{rect.width + 8}px" style:height="{rect.height + 8}px"></div>
    {/if}
  </div>
  <div class="card" bind:this={card} style:left="{pos.left}px" style:top="{pos.top}px" role="dialog" aria-label={step.title}>
    <div class="n">{(help.tourStep ?? 0) + 1} / {TOUR.length}</div>
    <b>{step.title}</b>
    <p>{step.body}</p>
    <div class="acts">
      <button type="button" class="skip" onclick={() => help.endTour()}>Skip tour</button>
      <span class="grow"></span>
      {#if help.tourStep}<button type="button" onclick={() => help.back()}>Back</button>{/if}
      <button type="button" class="primary" onclick={() => help.next()}>{help.tourStep === TOUR.length - 1 ? "Done" : "Next"}</button>
    </div>
  </div>
{/if}

<style>
  .veil {
    position: fixed;
    inset: 0;
    z-index: 950;
  }
  .hole {
    position: absolute;
    border-radius: var(--r-2);
    box-shadow: 0 0 0 9999px rgba(0, 0, 0, 0.55), 0 0 0 2px var(--accent);
    transition: all 180ms var(--ease-out);
    pointer-events: none;
  }
  .card {
    position: fixed;
    z-index: 951;
    width: 320px;
    padding: 12px 14px;
    background: var(--s2);
    border: 1px solid var(--line-strong);
    border-radius: var(--r-2);
    box-shadow: var(--shadow);
    transition: left 180ms var(--ease-out), top 180ms var(--ease-out);
  }
  .n {
    font-family: var(--font-code);
    font-size: 10.5px;
    color: var(--fg-3);
  }
  b {
    display: block;
    margin: 2px 0 4px;
    font-size: 14px;
  }
  p {
    margin: 0;
    font-size: 12.5px;
    line-height: 1.5;
    color: var(--fg-2);
  }
  .acts {
    display: flex;
    align-items: center;
    gap: 6px;
    margin-top: 12px;
  }
  .grow {
    flex: 1;
  }
  button {
    height: 26px;
    padding: 0 12px;
    font-size: 12px;
    font-weight: 600;
    color: var(--fg);
    border: 1px solid var(--line-strong);
    border-radius: var(--r-1);
  }
  .primary {
    color: var(--on-accent);
    background: var(--accent);
    border-color: var(--accent);
  }
  .skip {
    border: 0;
    padding: 0;
    color: var(--fg-3);
    font-weight: 500;
  }
</style>
