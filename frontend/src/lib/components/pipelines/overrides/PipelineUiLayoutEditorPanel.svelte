<script lang="ts">
  type Props = {
    open: boolean;
    panelStyle: string;
    outputs: string[];
    rows: number;
    columns: number;
    outputKeys: Record<string, string | null>;
    onClose: () => void;
    onStartDrag: (event: PointerEvent) => void;
    onSetDimensions: (rows: number, columns: number) => void;
    onSetOutputKey: (row: number, column: number, value: string | null) => void;
  };

  let {
    open,
    panelStyle,
    outputs,
    rows,
    columns,
    outputKeys,
    onClose,
    onStartDrag,
    onSetDimensions,
    onSetOutputKey
  }: Props = $props();

  const rowIndices = $derived.by(() => Array.from({ length: Math.max(0, Math.trunc(rows)) }, (_, idx) => idx));
  const columnIndices = $derived.by(() => Array.from({ length: Math.max(0, Math.trunc(columns)) }, (_, idx) => idx));

  function handleOutputDragStart(event: DragEvent, outputKey: string, sourceKey: string | null = null): void {
    if (!event.dataTransfer) return;
    event.dataTransfer.setData('application/helios-pipeline-output', outputKey);
    event.dataTransfer.setData('text/plain', outputKey);
    if (sourceKey) {
      event.dataTransfer.setData('application/helios-pipeline-output-source', sourceKey);
      event.dataTransfer.effectAllowed = 'move';
    } else {
      event.dataTransfer.effectAllowed = 'copy';
    }
  }

  function readDragPayload(event: DragEvent): { output: string | null; sourceKey: string | null } {
    const outputRaw =
      event.dataTransfer?.getData('application/helios-pipeline-output') ||
      event.dataTransfer?.getData('text/plain') ||
      '';
    const output = outputRaw.trim();
    const sourceKey = event.dataTransfer?.getData('application/helios-pipeline-output-source') || null;
    return { output: output.length ? output : null, sourceKey: sourceKey?.length ? sourceKey : null };
  }

  function handleDrop(event: DragEvent, row: number, column: number): void {
    event.preventDefault();
    event.stopPropagation();
    const { output, sourceKey } = readDragPayload(event);
    if (!output) return;
    onSetOutputKey(row, column, output);
    if (sourceKey && sourceKey !== `${row}:${column}`) {
      const [rowRaw, colRaw] = sourceKey.split(':');
      const sourceRow = Math.trunc(Number(rowRaw));
      const sourceCol = Math.trunc(Number(colRaw));
      if (Number.isInteger(sourceRow) && Number.isInteger(sourceCol)) {
        onSetOutputKey(sourceRow, sourceCol, null);
      }
    }
  }
</script>

{#if open}
  <div class="fixed z-[60] w-[32rem] max-w-[95vw]" style={panelStyle} data-floating-panel="layout">
    <div class="flex max-h-[calc(100vh-2rem)] max-h-[calc(100svh-2rem)] max-h-[calc(100dvh-2rem)] flex-col rounded border border-surface-800/80 bg-surface-950/95 shadow-2xl shadow-black/50">
      <div
        class="flex cursor-move items-center justify-between gap-2 border-b border-surface-800/70 p-3"
        role="presentation"
        onpointerdown={onStartDrag}
      >
        <div>
          <p class="text-micro-tight uppercase tracking-[0.2em] text-surface-500">Layout Builder</p>
          <p class="text-xs text-surface-400">Drag outputs into the grid.</p>
        </div>
        <button class="btn btn-3xs preset-outline" type="button" onclick={onClose}>Close</button>
      </div>

      <div class="min-h-0 flex-1 space-y-4 overflow-auto p-3">
        <div>
          <p class="text-micro-tight uppercase tracking-[0.3em] text-surface-500">Outputs</p>
          {#if outputs.length === 0}
            <p class="mt-2 text-xs text-surface-500">No outputs found for this pipeline.</p>
          {:else}
            <div class="mt-2 flex gap-2 overflow-x-auto pb-1">
              {#each outputs as output (output)}
                <div
                  class="shrink-0 cursor-grab rounded border border-surface-800/70 bg-surface-900/60 px-2 py-1 text-xs text-surface-200 hover:border-primary-500/40"
                  role="button"
                  aria-label={`Drag output ${output}`}
                  tabindex="0"
                  draggable="true"
                  ondragstart={(event) => handleOutputDragStart(event, output)}
                >
                  {output}
                </div>
              {/each}
            </div>
          {/if}
        </div>

        <div>
          <div class="flex flex-wrap items-center justify-between gap-2">
            <div>
              <p class="text-micro-tight uppercase tracking-[0.3em] text-surface-500">Grid</p>
              <p class="mt-1 text-xs text-surface-400">Rows and columns mirror the stream layout.</p>
            </div>
            <button class="btn btn-3xs preset-tonal uppercase tracking-[0.3em]" type="button" onclick={() => onSetDimensions(1, 1)}>
              1×1
            </button>
          </div>

          <div class="mt-3 grid gap-3 sm:grid-cols-2">
            <label class="text-sm">
              <span class="text-2xs uppercase tracking-[0.3em] text-surface-500">Rows</span>
              <input
                class="mt-1 w-full rounded border border-surface-700 bg-surface-900/70 px-3 py-2"
                type="number"
                min="1"
                max="6"
                value={rows}
                onchange={(event) => onSetDimensions(Number(event.currentTarget.value), columns)}
              />
            </label>
            <label class="text-sm">
              <span class="text-2xs uppercase tracking-[0.3em] text-surface-500">Columns</span>
              <input
                class="mt-1 w-full rounded border border-surface-700 bg-surface-900/70 px-3 py-2"
                type="number"
                min="1"
                max="6"
                value={columns}
                onchange={(event) => onSetDimensions(rows, Number(event.currentTarget.value))}
              />
            </label>
          </div>

          <div
            class="mt-4 grid gap-2"
            style={`grid-template-columns: repeat(${columns}, minmax(0, 1fr)); grid-template-rows: repeat(${rows}, minmax(120px, 1fr)); min-height: ${Math.max(1, Math.trunc(rows)) * 120}px;`}
            role="grid"
            aria-label="Pipeline output layout grid"
          >
            {#each rowIndices as row (row)}
              {#each columnIndices as column (column)}
                {@const key = `${row}:${column}`}
                {@const output = outputKeys[key] ?? null}
                <div
                  class="group relative overflow-hidden rounded border border-surface-800/70 bg-surface-900/40 p-3 text-xs text-surface-200"
                  role="gridcell"
                  tabindex="0"
                  ondragover={(event) => event.preventDefault()}
                  ondrop={(event) => handleDrop(event, row, column)}
                >
                  <div class="absolute right-2 top-2">
                    {#if output}
                      <button
                        class="btn btn-3xs preset-tonal uppercase tracking-[0.3em] opacity-0 group-hover:opacity-100"
                        type="button"
                        onclick={(event) => {
                          event.stopPropagation();
                          onSetOutputKey(row, column, null);
                        }}
                      >
                        Clear
                      </button>
                    {/if}
                  </div>
                  <div class="flex h-full items-center justify-center text-center">
                    {#if output}
                      <div
                        class="cursor-grab rounded border border-surface-800/70 bg-surface-950/60 px-3 py-2"
                        role="button"
                        aria-label={`Drag output ${output}`}
                        tabindex="0"
                        draggable="true"
                        ondragstart={(event) => handleOutputDragStart(event, output, key)}
                      >
                        {output}
                      </div>
                    {:else}
                      <span class="text-micro-tight uppercase tracking-[0.3em] text-surface-500">Drop output</span>
                    {/if}
                  </div>
                </div>
              {/each}
            {/each}
          </div>
        </div>
      </div>
    </div>
  </div>
{/if}
