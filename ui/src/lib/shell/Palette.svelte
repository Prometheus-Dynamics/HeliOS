<script lang="ts">
  // Ctrl+K: screens, panes, cameras, pipelines, devices, actions, and plain
  // descriptions of problems ("image too dark").
  import Icon from "$lib/components/common/Icon.svelte";
  import { commands, type Command } from "$lib/core/commands.svelte";

  let query = $state("");
  let index = $state(0);
  let input = $state<HTMLInputElement>();
  const results = $derived(commands.open ? commands.search(query) : []);

  $effect(() => {
    if (commands.open) {
      query = "";
      index = 0;
      queueMicrotask(() => input?.focus());
    }
  });
  $effect(() => {
    void query;
    index = 0;
  });

  function run(c: Command | undefined) {
    if (!c) return;
    commands.open = false;
    c.run();
  }

  function keydown(event: KeyboardEvent) {
    if (event.key === "ArrowDown") {
      event.preventDefault();
      index = Math.min(results.length - 1, index + 1);
    } else if (event.key === "ArrowUp") {
      event.preventDefault();
      index = Math.max(0, index - 1);
    } else if (event.key === "Enter") run(results[index]);
    else if (event.key === "Escape") commands.open = false;
  }

  const grouped = $derived.by(() => {
    const out: { group: string; items: { c: Command; i: number }[] }[] = [];
    results.forEach((c, i) => {
      let g = out.find((x) => x.group === c.group);
      if (!g) out.push((g = { group: c.group, items: [] }));
      g.items.push({ c, i });
    });
    return out;
  });
</script>

{#if commands.open}
  <div class="scrim" role="presentation" onpointerdown={() => (commands.open = false)}></div>
  <div class="palette" role="dialog" aria-label="Command palette">
    <div class="q">
      <Icon name="search" size={15} />
      <input bind:this={input} bind:value={query} onkeydown={keydown} placeholder="Screen, pane, camera, action, or a problem (“image too dark”)" aria-label="Search" />
      <kbd>Esc</kbd>
    </div>
    <div class="results">
      {#each grouped as g (g.group)}
        <div class="group">{g.group}</div>
        {#each g.items as { c, i } (c.id)}
          <button type="button" class="row" class:on={i === index} onpointermove={() => (index = i)} onclick={() => run(c)}>
            <span class="ic"><Icon name={c.icon ?? "arrow-right"} size={14} /></span>
            <span class="t">{c.title}</span>
            {#if c.shortcut}<kbd>{c.shortcut}</kbd>{/if}
          </button>
        {/each}
      {:else}
        <div class="none">No match for “{query}”.</div>
      {/each}
    </div>
  </div>
{/if}

<style>
  .scrim {
    position: fixed;
    inset: 0;
    z-index: 900;
    background: rgba(0, 0, 0, 0.3);
  }
  .palette {
    position: fixed;
    z-index: 901;
    top: 12vh;
    left: 50%;
    transform: translateX(-50%);
    width: min(620px, calc(100vw - 32px));
    background: var(--s2);
    border: 1px solid var(--line-strong);
    border-radius: var(--r-3);
    box-shadow: var(--shadow);
    overflow: hidden;
  }
  .q {
    display: flex;
    align-items: center;
    gap: 10px;
    height: 42px;
    padding: 0 12px;
    color: var(--fg-3);
    border-bottom: 1px solid var(--line);
  }
  .q input {
    flex: 1;
    background: transparent;
    border: 0;
    outline: 0;
    font: inherit;
    font-size: 14px;
    color: var(--fg);
  }
  .results {
    max-height: 56vh;
    overflow-y: auto;
    padding: 4px;
  }
  .group {
    padding: 8px 10px 3px;
    font-size: 10.5px;
    font-weight: 600;
    letter-spacing: 0.05em;
    text-transform: uppercase;
    color: var(--fg-3);
  }
  .row {
    display: flex;
    align-items: center;
    gap: 10px;
    width: 100%;
    height: 28px;
    padding: 0 10px;
    border-radius: var(--r-1);
    text-align: left;
    color: var(--fg);
    font-size: 13px;
  }
  .row.on {
    background: var(--accent-tint-strong);
  }
  .ic {
    display: inline-flex;
    color: var(--fg-2);
  }
  .t {
    flex: 1;
  }
  kbd {
    font-family: var(--font-code);
    font-size: 10.5px;
    color: var(--fg-3);
  }
  .none {
    padding: 18px;
    color: var(--fg-3);
    text-align: center;
  }
</style>
