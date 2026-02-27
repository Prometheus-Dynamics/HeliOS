<script lang="ts">
  import type { EdgeSelection } from './types';
  import type { PipelineConnectionRoute, PipelineConnectionStyle } from '$lib/types/pipeline';

  type StyleOption = { id: PipelineConnectionRoute; label: string; icon: string };

  type Props = {
    selectedEdge: EdgeSelection | null;
    selectedEdgeId: string | null;
    edgeStyleOptions: StyleOption[];
    currentStyle: PipelineConnectionStyle;
    onApplyStyle: (style: PipelineConnectionStyle) => void;
  };

  const { selectedEdge, selectedEdgeId, edgeStyleOptions, currentStyle, onApplyStyle }: Props = $props();
</script>

{#if selectedEdge && selectedEdgeId === selectedEdge.id}
  <div class="edge-style-menu">
    {#each edgeStyleOptions as option (option.id)}
      <button
        type="button"
        class={`edge-style-button ${currentStyle.route === option.id ? 'edge-style-button--active' : ''}`}
        title={`Set ${option.label} style`}
        onclick={() => onApplyStyle({ ...currentStyle, route: option.id })}
      >
        <span aria-hidden="true">{option.icon}</span>
      </button>
    {/each}
  </div>
{/if}
