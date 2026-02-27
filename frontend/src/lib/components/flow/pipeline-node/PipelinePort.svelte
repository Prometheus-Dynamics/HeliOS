<script lang="ts">
  import { Handle, Position } from '@xyflow/svelte';
  import type { PipelineDataType } from '$lib/types/pipeline';
  import type { PortInteractionHandlers, PortRenderInfo } from './types';
  import PortLabel from './PortLabel.svelte';
  import PortStatus from './PortStatus.svelte';
  import PortValueEditor from './PortValueEditor.svelte';

  const props = $props<{
    direction: 'input' | 'output';
    port: PortRenderInfo;
    handlers: PortInteractionHandlers;
    resolvePortStateClass: (direction: 'input' | 'output', type: PipelineDataType) => string;
    pixelColorInputAction: (node: HTMLInputElement, handleId: string) => { destroy: () => void };
  }>();

  const direction = $derived(props.direction);
  const port = $derived(props.port);
  const handlers = $derived(props.handlers);
  const resolvePortStateClass = $derived(props.resolvePortStateClass);
  const pixelColorInputAction = $derived(props.pixelColorInputAction);
  const isInput = $derived(direction === 'input');
  const portStateClass = $derived(resolvePortStateClass(direction, port.type));
  const ariaLabel = $derived(
    port.hasConstant
      ? `Configure ${direction} ${port.name} (constant value ${port.constantDisplay ?? 'unset'})`
      : `Configure ${direction} ${port.name}`
  );
  const issueTitle = $derived.by(() => (port.issueMessages && port.issueMessages.length ? port.issueMessages.join('\n') : null));
  const styleVars = $derived.by<string>(() => {
    const vars: string[] = [];
    if (port.color) vars.push(`--pipeline-port-color:${port.color}`);
    if (isInput && port.syncState?.color) vars.push(`--pipeline-port-sync-color:${port.syncState.color}`);
    return vars.length ? vars.join(';') : '';
  });
  const handleStyle = $derived.by<string>(() => {
    if (!port.color) return '';
    return `--pipeline-handle-color:${port.color}`;
  });
  const syncRole = $derived(isInput ? port.syncState?.role ?? undefined : undefined);
  const syncGroup = $derived(isInput ? port.syncState?.groupId ?? undefined : undefined);
  const containerClass = $derived(
    [
      'relative chip pipeline-port w-fit max-w-none rounded border transition',
      isInput ? 'pl-3 pr-2 text-left' : 'pl-2 pr-3 text-right',
      portStateClass,
      port.hasConstant ? 'pipeline-port--constant' : '',
      port.hasIssue ? 'pipeline-port--issue' : '',
      port.isHighlighted ? 'pipeline-port--highlight' : '',
      port.isSearchMatch ? 'pipeline-port--search-match' : '',
      isInput && port.syncState?.role === 'group' ? 'pipeline-port--sync-group' : '',
      isInput && port.syncState?.role === 'unsynced' ? 'pipeline-port--sync-unsynced' : ''
    ]
      .filter(Boolean)
      .join(' ')
  );

</script>

<div
  class={containerClass}
  data-port-kind={port.kind ?? undefined}
  data-port-has-color={port.color ? 'true' : undefined}
  data-port-custom={port.hasCustomColor ? 'true' : undefined}
  data-port-constant={port.hasConstant ? 'true' : undefined}
  data-port-issue={port.hasIssue ? 'true' : undefined}
  data-sync-role={syncRole}
  data-sync-group={syncGroup}
  style={styleVars}
  role="button"
  tabindex="0"
  aria-label={ariaLabel}
  title={issueTitle ?? undefined}
  onclick={(event) => event.stopPropagation()}
  onkeydown={(event) => {
    if (event.key === 'Enter' || event.key === ' ') {
      event.preventDefault();
      handlers.handlePortDoubleClick(direction, port, event);
    }
    event.stopPropagation();
  }}
  ondblclick={(event) => handlers.handlePortDoubleClick(direction, port, event)}
  oncontextmenu={(event) => {
    event.preventDefault();
    event.stopPropagation();
    handlers.handlePortContextMenu(direction, port, event);
  }}
>
  {#if isInput}
    <Handle id={port.handleId} type="target" position={Position.Left} style={handleStyle}>
      <span class="sr-only">Input {port.name}</span>
    </Handle>
  {/if}

  <div class={`flex flex-col py-1 ${isInput ? '' : 'items-end'}`}>
    <PortLabel {direction} {port} />
    <PortValueEditor {direction} {port} {handlers} {pixelColorInputAction} />
    <PortStatus {direction} {port} />
  </div>

  {#if !isInput}
    <Handle id={port.handleId} type="source" position={Position.Right} style={handleStyle}>
      <span class="sr-only">Output {port.name}</span>
    </Handle>
  {/if}
</div>

<style>
  .pipeline-port {
    --pipeline-port-color-base: var(--pipeline-port-color, var(--color-primary-300, #38bdf8));
    --pipeline-port-surface: var(--flow-surface, #0f172a);
    --pipeline-port-surface-soft: var(--flow-surface-soft, rgba(15, 23, 42, 0.82));
    --pipeline-port-border: var(--flow-border, #1f2937);
    --pipeline-port-text: var(--flow-text, #f8fafc);
  }

  .pipeline-port[data-port-has-color='true'] {
    border-color: color-mix(
      in srgb,
      var(--pipeline-port-color-base) 42%,
      var(--pipeline-port-border) 58%
    );
    background-color: color-mix(
      in srgb,
      var(--pipeline-port-color-base) 26%,
      var(--pipeline-port-surface-soft) 74%
    );
    color: color-mix(
      in srgb,
      var(--pipeline-port-text) 84%,
      var(--pipeline-port-color-base) 16%
    );
  }

  .pipeline-port[data-port-has-color='true'] :global(.pipeline-port__name) {
    color: color-mix(
      in srgb,
      var(--pipeline-port-text) 68%,
      var(--pipeline-port-color-base) 32%
    );
  }

  .pipeline-port--issue {
    border-color: color-mix(
      in srgb,
      #fbbf24 65%,
      var(--pipeline-port-border)
    );
    box-shadow:
      0 0 0 1px rgba(251, 191, 36, 0.35),
      0 0 12px rgba(251, 191, 36, 0.25);
  }

  .pipeline-port--issue :global(.pipeline-port__name) {
    color: color-mix(in srgb, #fde68a 80%, var(--pipeline-port-text) 20%);
  }

  .pipeline-port--highlight {
    border-color: color-mix(
      in srgb,
      var(--color-primary-200, #bae6fd) 65%,
      var(--pipeline-port-border)
    );
    box-shadow:
      0 0 0 1px rgba(56, 189, 248, 0.35),
      0 0 12px rgba(56, 189, 248, 0.4);
    animation: pipeline-port-focus 1s ease-in-out infinite;
  }

  .pipeline-port--search-match {
    border-color: color-mix(
      in srgb,
      var(--color-primary-200, #bae6fd) 55%,
      var(--pipeline-port-border)
    );
    box-shadow:
      0 0 0 1px rgba(56, 189, 248, 0.2),
      0 0 10px rgba(56, 189, 248, 0.25);
  }

  @keyframes pipeline-port-focus {
    0% {
      box-shadow:
        0 0 0 0 rgba(56, 189, 248, 0.4),
        0 0 8px rgba(56, 189, 248, 0.32);
    }
    50% {
      box-shadow:
        0 0 0 4px rgba(56, 189, 248, 0.15),
        0 0 18px rgba(56, 189, 248, 0.55);
    }
    100% {
      box-shadow:
        0 0 0 0 rgba(56, 189, 248, 0.4),
        0 0 8px rgba(56, 189, 248, 0.32);
    }
  }

  .pipeline-port[data-port-has-color='true'] :global(.pipeline-port__type-inline) {
    color: color-mix(
      in srgb,
      var(--pipeline-port-text) 78%,
      var(--pipeline-port-color-base) 22%
    );
  }

  .pipeline-port[data-port-has-color='true'] :global(.pipeline-port__detail) {
    color: color-mix(
      in srgb,
      var(--pipeline-port-text) 86%,
      var(--pipeline-port-color-base) 14%
    );
  }

  .pipeline-port[data-port-constant='true'] {
    --pipeline-port-constant-accent: color-mix(
      in srgb,
      var(--pipeline-port-color-base) 82%,
      var(--pipeline-port-border) 18%
    );
    --pipeline-port-constant-surface: color-mix(
      in srgb,
      var(--pipeline-port-color-base) 58%,
      var(--pipeline-port-surface-soft) 42%
    );
    --pipeline-port-constant-chip: color-mix(
      in srgb,
      var(--pipeline-port-color-base) 72%,
      var(--pipeline-port-surface) 28%
    );

    border-color: var(--pipeline-port-constant-accent);
    background-color: color-mix(
      in srgb,
      var(--pipeline-port-constant-surface) 90%,
      rgba(255, 255, 255, 0.02) 10%
    );
    box-shadow:
      0 0 0 1px color-mix(
        in srgb,
        var(--pipeline-port-constant-accent) 85%,
        transparent
      ),
      0 0 10px color-mix(
        in srgb,
        var(--pipeline-port-color-base) 55%,
        transparent
      );
  }

  :global(.pipeline-port__constant) {
    border: 1px solid color-mix(
      in srgb,
      var(--pipeline-port-constant-accent, var(--pipeline-port-color-base)) 80%,
      var(--pipeline-port-border) 20%
    );
    background-color: color-mix(
      in srgb,
      var(--pipeline-port-constant-chip, var(--pipeline-port-color-base)) 85%,
      rgba(255, 255, 255, 0.05) 15%
    );
    color: color-mix(
      in srgb,
      var(--pipeline-port-text) 40%,
      var(--pipeline-port-color-base) 60%
    );
    text-shadow: 0 0 1px color-mix(
      in srgb,
      var(--pipeline-port-color-base) 38%,
      transparent
    );
  }

  :global(.pipeline-port__constant-value) {
    color: color-mix(
      in srgb,
      var(--pipeline-port-text) 42%,
      var(--pipeline-port-color-base) 58%
    );
  }

  :global(.pipeline-port__constant--default) {
    opacity: 0.78;
    filter: saturate(0.7);
  }

  :global(.pipeline-port__boolean) {
    border: 1px solid color-mix(
      in srgb,
      var(--pipeline-port-color-base) 26%,
      var(--pipeline-port-border) 74%
    );
    background-color: color-mix(
      in srgb,
      var(--pipeline-port-color-base) 10%,
      var(--pipeline-port-surface-soft) 90%
    );
    color: color-mix(in srgb, var(--pipeline-port-text) 92%, transparent);
  }

  :global(.pipeline-port__boolean-input) {
    accent-color: var(--pipeline-port-color-base);
  }

  :global(.pipeline-port__boolean-label) {
    letter-spacing: 0.12em;
  }

  :global(.pipeline-port__pixel-actions) {
    display: flex;
    align-items: center;
    gap: 0.35rem;
  }

  :global(.pipeline-port__pixel) {
    display: inline-flex;
    align-items: center;
    justify-content: space-between;
    border: 1px solid color-mix(
      in srgb,
      var(--pipeline-port-pixel, #ffffff) 22%,
      var(--pipeline-port-border) 78%
    );
    background-color: color-mix(
      in srgb,
      var(--pipeline-port-pixel, #ffffff) 14%,
      var(--pipeline-port-surface-soft) 86%
    );
    color: color-mix(
      in srgb,
      var(--pipeline-port-text) 85%,
      var(--pipeline-port-pixel, #ffffff) 15%
    );
    cursor: pointer;
    text-align: left;
  }

  :global(.pipeline-port__pixel--readonly) {
    cursor: default;
  }

  :global(.pipeline-port__pixel-swatch) {
    width: 1rem;
    height: 1rem;
    border-radius: 0.25rem;
    box-shadow: inset 0 0 0 1px rgba(15, 23, 42, 0.32);
    background-color: var(--pipeline-port-pixel, #ffffff);
  }

  :global(.pipeline-port__pixel-value) {
    display: inline-flex;
    flex-direction: row;
    gap: 0.3rem;
    align-items: baseline;
    letter-spacing: 0.12em;
  }

  :global(.pipeline-port__pixel-alpha) {
    font-size: 0.5rem;
    letter-spacing: 0.18em;
    color: color-mix(in srgb, var(--pipeline-port-text) 60%, transparent);
  }

  :global(.pipeline-port__pixel-input) {
    position: absolute;
    opacity: 0;
    pointer-events: none;
    width: 0;
    height: 0;
  }

  :global(.pipeline-port__pixel-wrapper) {
    position: relative;
  }

  :global(.pipeline-port__enum) {
    font-size: 0.55rem;
    letter-spacing: 0.12em;
    text-transform: uppercase;
    color: color-mix(in srgb, var(--pipeline-port-text) 72%, transparent);
  }

  :global(.pipeline-port__enum-label) {
    font-size: 0.5rem;
    letter-spacing: 0.1em;
    color: color-mix(in srgb, var(--pipeline-port-text) 65%, transparent);
  }

  :global(.pipeline-port__numeric) {
    border: 1px solid color-mix(
      in srgb,
      var(--pipeline-port-color-base) 18%,
      var(--pipeline-port-border) 82%
    );
    background-color: color-mix(
      in srgb,
      var(--pipeline-port-color-base) 8%,
      var(--pipeline-port-surface-soft) 92%
    );
    font-size: 0.68rem;
    letter-spacing: 0.06em;
    width: 100%;
  }

  :global(.pipeline-port__numeric-input) {
    width: 100%;
    min-width: 0;
    border-radius: 0.3rem;
    border: 1px solid color-mix(in srgb, var(--pipeline-port-border) 70%, transparent);
    background: color-mix(in srgb, var(--pipeline-port-surface) 90%, transparent);
    color: var(--pipeline-port-text);
    padding: 0.08rem 0.35rem;
    font-size: 0.68rem;
    font-family: inherit;
  }

  :global(.pipeline-port__numeric-input):focus {
    outline: 1px solid color-mix(in srgb, var(--pipeline-port-color-base) 72%, transparent);
  }

  :global(.pipeline-port__select) {
    width: 100%;
    min-width: 0;
    border-radius: 0.45rem;
    border: 1px solid color-mix(in srgb, var(--pipeline-port-border) 80%, transparent);
    background: color-mix(in srgb, var(--pipeline-port-surface) 92%, transparent);
    color: var(--pipeline-port-text);
    padding: 0.25rem 0.5rem;
    font-size: 0.75rem;
    font-family: inherit;
  }

  :global(.pipeline-port__select):focus {
    outline: 2px solid color-mix(in srgb, var(--pipeline-port-color-base) 72%, transparent);
  }

  .pipeline-port--sync-group {
    border-color: color-mix(
      in srgb,
      var(--pipeline-port-sync-color, var(--pipeline-port-color-base)) 85%,
      transparent
    );
    box-shadow:
      0 0 0 1px color-mix(
        in srgb,
        var(--pipeline-port-sync-color, var(--pipeline-port-color-base)) 55%,
        transparent
      ),
      inset 0 0 0 1px color-mix(
        in srgb,
        var(--pipeline-port-sync-color, var(--pipeline-port-color-base)) 30%,
        transparent
      );
  }

  .pipeline-port--sync-group :global(.pipeline-port__name) {
    color: color-mix(
      in srgb,
      var(--pipeline-port-sync-color, var(--pipeline-port-color-base)) 80%,
      var(--pipeline-port-text) 20%
    );
  }

  .pipeline-port--sync-unsynced {
    border-style: dashed;
    opacity: 0.65;
  }

  :global(.pipeline-port__name) {
    display: inline-flex;
    align-items: baseline;
    gap: 0.4rem;
    max-width: 100%;
  }

  :global(.pipeline-port__name-text) {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  :global(.pipeline-port__type-inline) {
    flex: 0 0 auto;
    color: color-mix(in srgb, currentColor 70%, transparent);
    font-weight: 600;
    text-transform: none;
    letter-spacing: 0.02em;
  }
</style>
