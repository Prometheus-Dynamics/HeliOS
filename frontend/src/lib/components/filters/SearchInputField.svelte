<script lang="ts">
  import { createEventDispatcher, onMount } from 'svelte';

  type Size = 'default' | 'compact';

  type SearchInputFieldProps = {
    value?: string;
    placeholder?: string;
    ariaLabel?: string;
    size?: Size;
    autofocus?: boolean;
  };

  let {
    value = $bindable(''),
    placeholder = 'Search',
    ariaLabel,
    size = 'compact',
    autofocus = false
  }: SearchInputFieldProps = $props();

  const dispatch = createEventDispatcher<{ clear: void }>();
  let inputElement: HTMLInputElement | null = null;

  onMount(() => {
    if (autofocus) {
      queueMicrotask(() => inputElement?.focus());
    }
  });

  function clearValue() {
    if (!value) return;
    value = '';
    dispatch('clear');
    inputElement?.focus();
  }

  const inputClass = $derived.by(() =>
    `flex-1 border-none bg-transparent ${size === 'compact' ? 'text-[0.74rem]' : 'text-sm'} text-surface-100 placeholder:text-surface-500 outline-none focus:outline-none focus-visible:outline-none focus:ring-0`
  );

  export type $$Props = SearchInputFieldProps;
</script>

<div
  class={`group relative flex w-full items-center gap-2 rounded border border-surface-700/60 bg-surface-950/40 text-surface-300 shadow-inner transition focus-within:border-primary-400/70 focus-within:text-surface-100 ${
    size === 'compact' ? 'px-2.5 py-1.5' : 'px-4 py-2'
  }`}
>
  <svg
    class={`${size === 'compact' ? 'h-3.5 w-3.5' : 'h-5 w-5'} text-surface-500`}
    viewBox="0 0 20 20"
    fill="none"
    stroke="currentColor"
    stroke-width="1.5"
    aria-hidden="true"
  >
    <path
      stroke-linecap="round"
      stroke-linejoin="round"
      d="m13.5 12.5 3.5 3.5m-2-5.5a5.5 5.5 0 1 1-11 0 5.5 5.5 0 0 1 11 0Z"
    />
  </svg>
  <input
    bind:this={inputElement}
    type="search"
    class={inputClass}
    {placeholder}
    bind:value
    aria-label={ariaLabel}
    autocomplete="off"
    autocapitalize="none"
    spellcheck="false"
  />
  {#if value.trim().length}
    <button
      type="button"
      class={`rounded-full text-surface-500 transition hover:text-surface-200 focus-visible:outline focus-visible:outline-2 focus-visible:outline-primary-400 ${
        size === 'compact' ? 'p-1' : 'p-1.5'
      }`}
      aria-label="Clear search"
      onclick={clearValue}
    >
      <svg
        class={`${size === 'compact' ? 'h-3 w-3' : 'h-4 w-4'}`}
        viewBox="0 0 20 20"
        fill="none"
        stroke="currentColor"
        stroke-width="1.8"
        aria-hidden="true"
      >
        <path stroke-linecap="round" stroke-linejoin="round" d="m6 6 8 8M6 14l8-8" />
      </svg>
    </button>
  {/if}
</div>
