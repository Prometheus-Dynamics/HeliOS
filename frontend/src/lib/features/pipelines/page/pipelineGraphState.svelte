<script lang="ts" module>
  import { onMount, onDestroy } from 'svelte';
  import { get, type Readable } from 'svelte/store';
  import type { PipelineGraphEdgeSelection } from '$lib';

  type GraphStateParams = {
    graphContextMenu: Readable<{ visible: boolean }>;
    graphSelection: Readable<{ nodeId: string | null; nodes: string[]; edge: { id: string } | null }>;
    closeGraphContextMenu: () => void;
    removeGraphNode: (nodeId: string | null) => void;
    removeGraphConnection: (edge: PipelineGraphEdgeSelection) => void;
  };

  export function setupPipelineGraphState(params: GraphStateParams) {
    const { graphContextMenu, graphSelection, closeGraphContextMenu, removeGraphNode, removeGraphConnection } = params;
    const state = $state({
      graphContextMenuElement: null as HTMLDivElement | null
    });

    onMount(() => {
      const dismiss = () => closeGraphContextMenu();
      const handleScroll = (event: Event) => {
        if (!get(graphContextMenu).visible) return;
        const target = event.target as Node | null;
        if (target && state.graphContextMenuElement?.contains(target)) {
          return;
        }
        dismiss();
      };
      const handleKeyDown = (event: KeyboardEvent) => {
        if (event.key === 'Escape') {
          closeGraphContextMenu();
          return;
        }
        if (event.key !== 'Delete' && event.key !== 'Backspace') {
          return;
        }
        const target = event.target as HTMLElement | null;
        if (target) {
          const tagName = target.tagName;
          const editable = target.isContentEditable;
          if (editable || tagName === 'INPUT' || tagName === 'TEXTAREA' || tagName === 'SELECT') {
            return;
          }
        }
        const selection = get(graphSelection);
        const fallbackNodeId = !selection.nodeId && selection.nodes.length === 1 ? selection.nodes[0] : null;
        if (selection.nodeId || fallbackNodeId) {
          event.preventDefault();
          event.stopPropagation();
          removeGraphNode(selection.nodeId ?? fallbackNodeId);
          return;
        }
        if (selection.edge) {
          event.preventDefault();
          event.stopPropagation();
          removeGraphConnection(selection.edge as PipelineGraphEdgeSelection);
        }
      };
      window.addEventListener('scroll', handleScroll, true);
      window.addEventListener('resize', dismiss);
      window.addEventListener('keydown', handleKeyDown, true);
      return () => {
        window.removeEventListener('scroll', handleScroll, true);
        window.removeEventListener('resize', dismiss);
        window.removeEventListener('keydown', handleKeyDown, true);
      };
    });

    onDestroy(() => {
      closeGraphContextMenu();
    });

    return state;
  }
</script>
