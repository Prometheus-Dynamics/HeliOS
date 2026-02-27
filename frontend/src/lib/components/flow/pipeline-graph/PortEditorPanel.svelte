<script lang="ts">
  import { tick, onDestroy } from 'svelte';
  import { parseEnumVariant } from '$lib/features/pipelines/valueFormatting';
  import ColorDropperButton from '$lib/components/controls/ColorDropperButton.svelte';
  import type { PortEditorState } from './types';

  let {
    portEditor,
    draft = $bindable(''),
    error = null,
    isPixelPortEditor = false,
    pixelEditorHex = '#ffffff',
    pixelEditorAlpha = 255,
    onApply = () => {},
    onClear = () => {},
    onClose = () => {},
    onColorInput = () => {}
  } = $props<{
    portEditor: PortEditorState;
    draft?: string;
    error?: string | null;
    isPixelPortEditor?: boolean;
    pixelEditorHex?: string;
    pixelEditorAlpha?: number;
    onApply?: () => void;
    onClear?: () => void;
    onClose?: () => void;
    onColorInput?: (event: Event) => void;
  }>();

  let portEditorInput = $state<HTMLTextAreaElement | HTMLSelectElement | HTMLInputElement | null>(null);
  let colorPickerHandle: number | null = null;

  $effect(() => {
    if (!portEditor || !portEditorInput) return;
    void tick().then(() => {
      if (!portEditorInput) return;
      portEditorInput.focus();
      if (portEditorInput instanceof HTMLTextAreaElement) {
        portEditorInput.select();
      }
      if (
        isPixelPortEditor &&
        portEditorInput instanceof HTMLInputElement &&
        portEditorInput.type === 'color'
      ) {
        const input = portEditorInput as HTMLInputElement & { showPicker?: () => void };
        const openPicker = () => {
          if (typeof input.showPicker === 'function') {
            input.showPicker();
          } else {
            input.click();
          }
        };
        if (colorPickerHandle != null) {
          cancelAnimationFrame(colorPickerHandle);
        }
        colorPickerHandle = requestAnimationFrame(openPicker);
      }
    });
  });

  onDestroy(() => {
    if (colorPickerHandle != null) {
      cancelAnimationFrame(colorPickerHandle);
    }
  });

  const applyDropperColor = (hex: string) => {
    const input = portEditorInput;
    if (!input || !(input instanceof HTMLInputElement) || input.type !== 'color') return;
    input.value = hex;
    onColorInput?.({ currentTarget: input } as unknown as Event);
  };
</script>

<button
  type="button"
  class="port-editor__backdrop"
  onclick={onClose}
  aria-label="Dismiss port editor"
></button>
<div
  class="port-editor__panel"
  style={`top:${portEditor.anchor.y}px; left:${portEditor.anchor.x}px`}
  role="dialog"
  aria-modal="true"
>
  <form
    class="port-editor__form"
    onsubmit={(event) => {
      event.preventDefault();
      onApply();
    }}
  >
    <header class="port-editor__header">
      <h3 class="port-editor__title">Set port value</h3>
      <p class="port-editor__subtitle">
        {#if portEditor.mode === 'node'}
          Node {portEditor.nodeId.slice(0, 8)} · {portEditor.port}
        {:else}
          Pipeline input · {portEditor.port}
        {/if}
      </p>
      <p class="port-editor__datatype">Type: {portEditor.dataTypeKey ?? 'Unknown'}</p>
    </header>
    <label class="port-editor__label">
      <span>Value</span>
      {#if portEditor.variants.length > 0}
        <select
          class="port-editor__select"
          bind:this={portEditorInput}
          bind:value={draft}
          disabled={!portEditor.settable}
        >
          {#each portEditor.variants as variant (variant)}
            {@const parsedVariant = parseEnumVariant(variant)}
            <option value={variant} title={parsedVariant.value}>
              {parsedVariant.label}
            </option>
          {/each}
        </select>
      {:else if isPixelPortEditor}
        <div class="port-editor__color">
          <div class="port-editor__color-actions">
            <input
              type="color"
              class="port-editor__color-input"
              value={pixelEditorHex}
              bind:this={portEditorInput}
              disabled={!portEditor.settable}
              oninput={onColorInput}
            />
            <ColorDropperButton
              title="Pick color from screen"
              ariaLabel="Pick color from screen"
              disabled={!portEditor.settable}
              onPick={applyDropperColor}
            />
          </div>
          <div class="port-editor__color-meta">
            <span class="port-editor__color-value">{pixelEditorHex.toUpperCase()}</span>
            <span class="port-editor__color-alpha">Alpha {pixelEditorAlpha}</span>
          </div>
        </div>
      {:else}
        <textarea
          class="port-editor__textarea"
          rows="3"
          bind:this={portEditorInput}
          bind:value={draft}
          placeholder={portEditor.settable ? 'Enter value' : 'Port is not settable'}
          disabled={!portEditor.settable}
        ></textarea>
      {/if}
    </label>
    {#if error}
      <p class="port-editor__error">{error}</p>
    {/if}
    {#if !portEditor.settable}
      <p class="port-editor__hint">This port does not accept manual values.</p>
    {/if}
    <div class="port-editor__actions">
      <button
        class="btn btn-3xs preset-outline uppercase tracking-[0.3em]"
        type="submit"
        disabled={!portEditor.settable}
      >
        Apply
      </button>
      <button
        class="btn btn-3xs preset-ghost uppercase tracking-[0.3em]"
        type="button"
        onclick={onClear}
        disabled={!portEditor.existingValue}
      >
        Clear
      </button>
    </div>
  </form>
</div>

<style>
  .port-editor__backdrop {
    position: absolute;
    inset: 0;
    background: linear-gradient(135deg, rgba(15, 23, 42, 0.5), rgba(15, 23, 42, 0.7));
    border-radius: inherit;
    border: none;
    cursor: pointer;
  }

  .port-editor__panel {
    position: fixed;
    z-index: 30;
    min-width: 18rem;
    max-width: min(24rem, calc(100vw - 1rem));
    background: var(--flow-surface, #0f172a);
    border: 1px solid var(--flow-border, #1f2937);
    border-radius: 0.65rem;
    box-shadow:
      0 20px 40px rgba(0, 0, 0, 0.35),
      0 0 0 1px rgba(255, 255, 255, 0.03);
    overflow: hidden;
  }

  .port-editor__form {
    display: flex;
    flex-direction: column;
    gap: 0.75rem;
    padding: 0.9rem;
  }

  .port-editor__header {
    display: flex;
    flex-direction: column;
    gap: 0.1rem;
  }

  .port-editor__title {
    font-size: 0.95rem;
    font-weight: 700;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    color: var(--flow-text, #f8fafc);
  }

  .port-editor__subtitle {
    font-size: 0.78rem;
    color: color-mix(in srgb, var(--flow-text, #f8fafc) 80%, transparent);
    letter-spacing: 0.08em;
  }

  .port-editor__datatype {
    font-size: 0.7rem;
    letter-spacing: 0.12em;
    text-transform: uppercase;
    color: color-mix(in srgb, var(--flow-text, #f8fafc) 65%, transparent);
  }

  .port-editor__label {
    display: flex;
    flex-direction: column;
    gap: 0.4rem;
    color: color-mix(in srgb, var(--flow-text, #f8fafc) 78%, transparent);
    font-weight: 600;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    font-size: 0.7rem;
  }

  .port-editor__textarea {
    width: 100%;
    min-height: 5rem;
    border-radius: 0.5rem;
    border: 1px solid var(--flow-border, #1f2937);
    background: color-mix(in srgb, var(--flow-surface, #0f172a) 90%, transparent);
    color: var(--flow-text, #f8fafc);
    padding: 0.55rem;
    font-size: 0.9rem;
    font-family: inherit;
    resize: vertical;
  }

  .port-editor__select {
    width: 100%;
    border-radius: 0.5rem;
    border: 1px solid var(--flow-border, #1f2937);
    background: color-mix(in srgb, var(--flow-surface, #0f172a) 92%, transparent);
    color: var(--flow-text, #f8fafc);
    padding: 0.45rem 0.6rem;
    font-size: 0.9rem;
    font-family: inherit;
  }

  .port-editor__color {
    display: flex;
    align-items: center;
    gap: 0.75rem;
  }

  .port-editor__color-actions {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    flex-shrink: 0;
  }

  .port-editor__color-input {
    width: 3.5rem;
    min-width: 3.5rem;
    min-height: 3rem;
    border-radius: 0.5rem;
    border: 1px solid color-mix(in srgb, var(--flow-border, #1f2937) 92%, transparent);
    background: color-mix(in srgb, var(--flow-surface, #0f172a) 90%, transparent);
    cursor: pointer;
  }

  .port-editor__color-input:disabled {
    opacity: 0.6;
    cursor: not-allowed;
  }

  .port-editor__color-meta {
    display: flex;
    flex-direction: column;
    gap: 0.2rem;
    color: color-mix(in srgb, var(--flow-text, #f8fafc) 85%, transparent);
    flex: 1;
  }

  .port-editor__color-value {
    font-weight: 700;
    letter-spacing: 0.12em;
  }

  .port-editor__color-alpha {
    font-size: 0.78rem;
    letter-spacing: 0.14em;
    color: color-mix(in srgb, var(--flow-text, #f8fafc) 65%, transparent);
  }

  .port-editor__textarea:focus,
  .port-editor__select:focus {
    outline: 2px solid color-mix(
      in srgb,
      var(--flow-accent, var(--color-primary-300, #38bdf8)) 75%,
      transparent
    );
  }

  .port-editor__error {
    color: #fca5a5;
    font-size: 0.85rem;
    font-weight: 600;
  }

  .port-editor__hint {
    color: color-mix(in srgb, var(--flow-text, #f8fafc) 65%, transparent);
    font-size: 0.82rem;
  }

  .port-editor__actions {
    display: flex;
    gap: 0.65rem;
    justify-content: flex-end;
  }
</style>
