<script lang="ts">
  import PortEditorPanel from './PortEditorPanel.svelte';
  import type { PortEditorState } from './types';

  type Props = {
    portEditor: PortEditorState | null;
    draft: string;
    error: string | null;
    isPixelPortEditor: boolean;
    pixelEditorHex: string;
    pixelEditorAlpha: number;
    onApply: () => void;
    onClear: () => void;
    onClose: () => void;
    onColorInput: (event: Event) => void;
  };

  let {
    portEditor,
    draft = $bindable(''),
    error,
    isPixelPortEditor,
    pixelEditorHex,
    pixelEditorAlpha,
    onApply,
    onClear,
    onClose,
    onColorInput
  }: Props = $props();
</script>

{#if portEditor}
  <PortEditorPanel
    portEditor={portEditor}
    bind:draft={draft}
    {error}
    {isPixelPortEditor}
    {pixelEditorHex}
    {pixelEditorAlpha}
    onApply={onApply}
    onClear={onClear}
    onClose={onClose}
    onColorInput={onColorInput}
  />
{/if}
