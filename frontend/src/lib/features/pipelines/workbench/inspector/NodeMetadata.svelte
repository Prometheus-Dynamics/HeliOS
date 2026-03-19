<script lang="ts">
  import InspectorSection from '$lib/components/pipelines/inspector/InspectorSection.svelte';

  let {
    canEditNodeMetadata = false,
    nodeNameDraft = $bindable(''),
    nodeSummaryDraft = $bindable(''),
    metadataDirty = false,
    onApply
  }: {
    canEditNodeMetadata?: boolean;
    nodeNameDraft?: string;
    nodeSummaryDraft?: string;
    metadataDirty?: boolean;
    onApply?: () => void;
  } = $props();
</script>

{#if canEditNodeMetadata}
  <InspectorSection title="Node metadata">
    <label class="flex flex-col gap-1 text-xs text-surface-300">
      <span class="text-micro uppercase tracking-[0.24em] text-surface-500">Name</span>
      <input
        class="input h-9 text-sm"
        placeholder="Node name"
        bind:value={nodeNameDraft}
        onkeydown={(event) => event.key === 'Enter' && onApply?.()}
      />
    </label>
    <label class="flex flex-col gap-1 text-xs text-surface-300">
      <span class="text-micro uppercase tracking-[0.24em] text-surface-500">Description</span>
      <textarea
        class="input min-h-[60px] text-sm"
        placeholder="Optional description"
        bind:value={nodeSummaryDraft}
        onkeydown={(event) => event.key === 'Enter' && (event.metaKey || event.ctrlKey) && onApply?.()}
      ></textarea>
    </label>
    <div class="flex items-center gap-2">
      <button
        class="btn btn-3xs preset-outline uppercase tracking-[0.28em]"
        type="button"
        onclick={() => onApply?.()}
        disabled={!metadataDirty}
      >
        Save
      </button>
      {#if !metadataDirty}
        <span class="text-micro text-surface-500">Up to date</span>
      {/if}
    </div>
  </InspectorSection>
{/if}
