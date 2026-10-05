<script lang="ts" generics="T extends string | number">
  // A compact select.
  let {
    value = $bindable(),
    options,
    label,
    disabled = false,
    width,
    onchange,
  }: {
    value: T;
    options: { value: T; label: string }[] | T[];
    label?: string;
    disabled?: boolean;
    width?: number;
    onchange?: (value: T) => void;
  } = $props();

  const items = $derived(options.map((o) => (typeof o === "object" ? o : { value: o, label: String(o) })) as { value: T; label: string }[]);
</script>

<select
  class="choice"
  aria-label={label}
  {disabled}
  style:width={width ? `${width}px` : undefined}
  value={String(value)}
  onchange={(e) => {
    const raw = (e.currentTarget as HTMLSelectElement).value;
    const hit = items.find((i) => String(i.value) === raw);
    if (hit) {
      value = hit.value;
      onchange?.(hit.value);
    }
  }}
>
  {#each items as item (String(item.value))}
    <option value={String(item.value)}>{item.label}</option>
  {/each}
</select>

<style>
  .choice {
    height: calc(var(--row) - 4px);
    min-width: 0;
    max-width: 100%;
    padding: 0 22px 0 7px;
    font: inherit;
    font-size: 12px;
    color: var(--fg);
    background:
      linear-gradient(45deg, transparent 50%, var(--fg-3) 50%) calc(100% - 11px) 50% / 4px 4px no-repeat,
      linear-gradient(135deg, var(--fg-3) 50%, transparent 50%) calc(100% - 7px) 50% / 4px 4px no-repeat,
      var(--inset);
    border: 1px solid var(--line);
    border-radius: var(--r-1);
    appearance: none;
  }
  .choice:focus {
    outline: none;
    border-color: var(--accent-ring);
  }
  .choice option {
    background: var(--s2);
  }
</style>
