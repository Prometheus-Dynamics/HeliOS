<script lang="ts">
  import ColorWheelPicker from '$lib/components/controls/ColorWheelPicker.svelte';
  import type { PipelineUiControl } from '$lib/features/pipelines/pipelineUiTypes';

  type GradientStop = {
    id: string;
    color: string;
    position: number;
  };

  type Props = {
    editMode: boolean;
    gradientEditorOpen: boolean;
    gradientEditorTarget: 'trackGradient' | 'trackFill' | null;
    gradientPanelStyle: string;
    gradientStops: GradientStop[];
    gradientAngle: number;
    onGradientTrackRef: (el: HTMLDivElement | null) => void;
    selectedGradientStop: GradientStop | null;
    colorPickerOpen: boolean;
    colorPickerTarget: 'thumbFill' | 'thumbBorder' | 'defaultColor' | null;
    colorPanelStyle: string;
    currentPickerColor: string;
    selectedItem: PipelineUiControl | null;
    onCloseGradient: () => void;
    onCloseColor: () => void;
    onStartFloatingDrag: (event: PointerEvent, panel: 'gradient' | 'color' | 'layout') => void;
    onGradientTrackPointerDown: (event: PointerEvent) => void;
    onStartGradientDrag: (event: PointerEvent, id: string) => void;
    onUpdateGradientTarget: (stops: GradientStop[], angle?: number) => void;
    onRemoveSelectedStop: () => void;
    onUpdateSelectedStopColor: (color: string) => void;
    onUpdateSelectedStopPosition: (position: number) => void;
    onUpdateSelectedControl: (patch: Partial<PipelineUiControl>) => void;
    buildGradientString: (angle: number, stops: GradientStop[]) => string;
  };

  const {
    editMode,
    gradientEditorOpen,
    gradientEditorTarget,
    gradientPanelStyle,
    gradientStops,
    gradientAngle,
    onGradientTrackRef,
    selectedGradientStop,
    colorPickerOpen,
    colorPickerTarget,
    colorPanelStyle,
    currentPickerColor,
    selectedItem,
    onCloseGradient,
    onCloseColor,
    onStartFloatingDrag,
    onGradientTrackPointerDown,
    onStartGradientDrag,
    onUpdateGradientTarget,
    onRemoveSelectedStop,
    onUpdateSelectedStopColor,
    onUpdateSelectedStopPosition,
    onUpdateSelectedControl,
    buildGradientString
  }: Props = $props();

  let gradientTrackEl = $state<HTMLDivElement | null>(null);

  $effect(() => {
    onGradientTrackRef(gradientTrackEl);
  });
</script>

{#if editMode && gradientEditorOpen}
  <div class="fixed z-[60] w-[26rem] max-w-[95vw]" style={gradientPanelStyle} data-floating-panel="gradient">
    <div class="flex max-h-[calc(100vh-2rem)] max-h-[calc(100svh-2rem)] max-h-[calc(100dvh-2rem)] flex-col rounded border border-surface-800/80 bg-surface-950/95 shadow-2xl shadow-black/50">
      <div
        class="flex cursor-move items-start justify-between gap-2 border-b border-surface-800/70 p-3"
        onpointerdown={(event) => onStartFloatingDrag(event, 'gradient')}
        role="button"
        tabindex="-1"
        aria-label="Drag gradient builder"
      >
        <div>
          <p class="text-micro-tight uppercase tracking-[0.2em] text-surface-500">Gradient Builder</p>
          <p class="text-xs text-surface-400">
            {gradientEditorTarget === 'trackFill' ? 'Track Fill' : 'Track Gradient'}
          </p>
        </div>
        <button class="btn btn-3xs preset-outline" type="button" onclick={onCloseGradient}>Close</button>
      </div>

      <div class="min-h-0 flex-1 space-y-3 overflow-auto p-3">
        <div>
          <div
            class="gradient-editor-track"
            bind:this={gradientTrackEl}
            style={`background:${buildGradientString(gradientAngle, gradientStops)};`}
            onpointerdown={onGradientTrackPointerDown}
            role="button"
            tabindex="-1"
            aria-label="Edit gradient stops"
          >
            {#each gradientStops as stop (stop.id)}
              <button
                class={`gradient-marker ${stop.id === selectedGradientStop?.id ? 'is-selected' : ''}`}
                style={`left:${stop.position}%; background:${stop.color};`}
                type="button"
                aria-label="Gradient stop"
                onpointerdown={(event) => onStartGradientDrag(event, stop.id)}
              ></button>
            {/each}
          </div>
          <p class="mt-2 text-micro-tight text-surface-500">
            Click to add a stop. Drag markers to adjust positions.
          </p>
        </div>

        <div class="space-y-1">
          <p class="text-micro-tight uppercase tracking-[0.2em] text-surface-500">Angle</p>
          <div class="flex items-center gap-2">
            <input
              class="flex-1"
              type="range"
              min="0"
              max="360"
              value={gradientAngle}
              oninput={(event) => onUpdateGradientTarget(gradientStops, Number(event.currentTarget.value))}
            />
            <input
              class="w-20 rounded border border-surface-700 bg-surface-900/70 px-2 py-1 text-xs"
              type="number"
              value={gradientAngle}
              oninput={(event) => onUpdateGradientTarget(gradientStops, Number(event.currentTarget.value))}
            />
          </div>
        </div>

        {#if selectedGradientStop}
          <div class="space-y-2">
            <div class="flex items-center justify-between gap-2">
              <p class="text-micro-tight uppercase tracking-[0.2em] text-surface-500">Selected stop</p>
              <button class="btn btn-3xs preset-outline" type="button" onclick={onRemoveSelectedStop} disabled={gradientStops.length <= 2}>
                Remove
              </button>
            </div>
            <div class="flex justify-center rounded border border-surface-800/70 bg-surface-900/50 p-2">
              <ColorWheelPicker
                value={selectedGradientStop.color}
                size={180}
                on:change={(event) => onUpdateSelectedStopColor(event.detail.value)}
              />
            </div>
            <div class="grid grid-cols-2 gap-2">
              <div class="space-y-1">
                <p class="text-micro-tight uppercase tracking-[0.2em] text-surface-500">Color</p>
                <input
                  class="w-full rounded border border-surface-700 bg-surface-900/70 px-2 py-1 text-xs"
                  value={selectedGradientStop.color}
                  oninput={(event) => onUpdateSelectedStopColor(event.currentTarget.value)}
                />
              </div>
              <div class="space-y-1">
                <p class="text-micro-tight uppercase tracking-[0.2em] text-surface-500">Position %</p>
                <input
                  class="w-full rounded border border-surface-700 bg-surface-900/70 px-2 py-1 text-xs"
                  type="number"
                  min="0"
                  max="100"
                  value={selectedGradientStop.position.toFixed(1)}
                  oninput={(event) => onUpdateSelectedStopPosition(Number(event.currentTarget.value))}
                />
              </div>
            </div>
          </div>
        {/if}
      </div>
    </div>
  </div>
{/if}

{#if editMode && colorPickerOpen}
  <div class="fixed z-[60] w-[22rem] max-w-[90vw]" style={colorPanelStyle} data-floating-panel="color">
    <div class="flex max-h-[calc(100vh-2rem)] max-h-[calc(100svh-2rem)] max-h-[calc(100dvh-2rem)] flex-col rounded border border-surface-800/80 bg-surface-950/95 shadow-2xl shadow-black/50">
      <div
        class="flex cursor-move items-start justify-between gap-2 border-b border-surface-800/70 p-3"
        onpointerdown={(event) => onStartFloatingDrag(event, 'color')}
        role="button"
        tabindex="-1"
        aria-label="Drag color picker"
      >
        <div>
          <p class="text-micro-tight uppercase tracking-[0.2em] text-surface-500">Color Picker</p>
          <p class="text-xs text-surface-400">{colorPickerTarget === 'thumbBorder' ? 'Thumb Border' : 'Thumb Fill'}</p>
        </div>
        <button class="btn btn-3xs preset-outline" type="button" onclick={onCloseColor}>Close</button>
      </div>
      <div class="min-h-0 flex-1 space-y-3 overflow-auto p-3">
        <div class="flex justify-center rounded border border-surface-800/70 bg-surface-900/50 p-2">
          <ColorWheelPicker
            value={currentPickerColor}
            size={200}
            on:change={(event) => {
              if (!selectedItem) return;
              if (colorPickerTarget === 'thumbBorder') {
                onUpdateSelectedControl({ thumbBorder: event.detail.value });
              } else if (colorPickerTarget === 'thumbFill') {
                onUpdateSelectedControl({ thumbFill: event.detail.value });
              } else if (colorPickerTarget === 'defaultColor') {
                onUpdateSelectedControl({ default: event.detail.value });
              }
            }}
          />
        </div>
        <input
          class="w-full rounded border border-surface-700 bg-surface-900/70 px-2 py-1 text-xs"
          value={currentPickerColor}
          oninput={(event) => {
            const value = event.currentTarget.value;
            if (!selectedItem) return;
            if (colorPickerTarget === 'thumbBorder') {
              onUpdateSelectedControl({ thumbBorder: value });
            } else if (colorPickerTarget === 'thumbFill') {
              onUpdateSelectedControl({ thumbFill: value });
            } else if (colorPickerTarget === 'defaultColor') {
              onUpdateSelectedControl({ default: value });
            }
          }}
        />
      </div>
    </div>
  </div>
{/if}

<style>
  .gradient-editor-track {
    position: relative;
    height: 2rem;
    border-radius: 999px;
    border: 1px solid rgba(30, 41, 59, 0.6);
    cursor: crosshair;
  }

  .gradient-marker {
    position: absolute;
    top: 50%;
    width: 1rem;
    height: 1rem;
    border-radius: 999px;
    border: 2px solid rgba(255, 255, 255, 0.9);
    transform: translate(-50%, -50%);
    box-shadow: 0 2px 8px rgba(0, 0, 0, 0.35);
  }

  .gradient-marker.is-selected {
    border-color: rgba(56, 189, 248, 0.95);
    box-shadow: 0 0 0 2px rgba(14, 116, 144, 0.6);
  }
</style>
