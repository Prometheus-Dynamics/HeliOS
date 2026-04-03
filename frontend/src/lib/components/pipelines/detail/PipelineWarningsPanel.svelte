<script lang="ts">
  import FaIcon from '$lib/components/icons/FaIcon.svelte';
  import { faCheck, faClipboard, faCopy, faXmark } from '@fortawesome/free-solid-svg-icons';
  import type { PipelineDiagnosticWarning } from '$lib/types/pipeline';

  type PipelineWarningsPanelProps = {
    open: boolean;
    warnings: PipelineDiagnosticWarning[];
    title?: string;
    emptyMessage?: string;
    helpMessage?: string;
    className?: string;
    onClose: () => void;
    onFocus: (warning: PipelineDiagnosticWarning) => void;
  };

  const {
    open,
    warnings,
    title = 'Graph warnings',
    emptyMessage = 'No issues reported by the validator.',
    helpMessage = 'Select a warning to focus its node in the canvas.',
    className = '',
    onClose,
    onFocus
  }: PipelineWarningsPanelProps = $props();

  const warningCount = $derived(warnings.length);
  const warningLabel = $derived(
    `${warningCount} warning${warningCount === 1 ? '' : 's'}`
  );
  const hasWarnings = $derived(warningCount > 0);
  let copiedAll = $state(false);
  let copiedWarningKey = $state<string | null>(null);
  let copiedAllTimeout: ReturnType<typeof setTimeout> | null = null;
  let copiedWarningTimeout: ReturnType<typeof setTimeout> | null = null;

  function warningDescriptor(warning: PipelineDiagnosticWarning): string {
    const location = warning.nodeId ? `Node ${warning.nodeId}` : 'Graph';
    const port = warning.port ? `Port ${warning.port}` : null;
    const target = port ? `${location} · ${port}` : location;
    return `${target}: ${warning.message}`;
  }

  function warningsSnapshot(warningsList: PipelineDiagnosticWarning[]): string {
    return warningsList
      .map((warning, index) => `${index + 1}. ${warningDescriptor(warning)}`)
      .join('\n');
  }

  async function copyText(value: string): Promise<boolean> {
    if (!value) return false;
    if (typeof navigator === 'undefined' || !navigator.clipboard?.writeText) return false;
    try {
      await navigator.clipboard.writeText(value);
      return true;
    } catch {
      return false;
    }
  }

  function markCopiedAll(): void {
    copiedAll = true;
    if (copiedAllTimeout) clearTimeout(copiedAllTimeout);
    copiedAllTimeout = setTimeout(() => {
      copiedAll = false;
    }, 1400);
  }

  function markCopiedWarning(key: string): void {
    copiedWarningKey = key;
    if (copiedWarningTimeout) clearTimeout(copiedWarningTimeout);
    copiedWarningTimeout = setTimeout(() => {
      if (copiedWarningKey === key) {
        copiedWarningKey = null;
      }
    }, 1400);
  }

  async function copyWarning(warning: PipelineDiagnosticWarning, key: string): Promise<void> {
    const copied = await copyText(warningDescriptor(warning));
    if (copied) markCopiedWarning(key);
  }

  async function copyAllWarnings(): Promise<void> {
    const copied = await copyText(warningsSnapshot(warnings));
    if (copied) markCopiedAll();
  }

  export type $$Props = PipelineWarningsPanelProps;
</script>

{#if open}
  <div class="pointer-events-none absolute bottom-4 right-4 z-30 flex max-w-full flex-col items-end">
    <div
      class={`pointer-events-auto w-full max-w-[24rem] rounded-lg border border-amber-700/70 bg-surface-950/95 p-3 text-sm text-amber-50 shadow-2xl shadow-black/40 backdrop-blur ${className}`.trim()}
      style="width: min(24rem, calc(100vw - 2rem));"
    >
      <div class="flex items-start gap-3">
        <div class="flex-1">
          <div class="text-[0.56rem] font-semibold uppercase tracking-[0.28em] text-amber-200">
            {title}
          </div>
          <div class="text-lg font-semibold text-amber-50">{warningLabel}</div>
          <p class="text-xs text-amber-100/80">{hasWarnings ? helpMessage : emptyMessage}</p>
        </div>
        <div class="flex flex-col items-end gap-2">
          {#if hasWarnings}
            <button
              type="button"
              class="flex items-center gap-1 rounded-full border border-amber-700/60 bg-amber-950/50 px-2.5 py-1 text-micro-tight font-semibold uppercase tracking-[0.2em] text-amber-100 transition hover:border-amber-400 hover:text-white focus-visible:outline focus-visible:outline-2 focus-visible:outline-amber-300"
              onclick={copyAllWarnings}
              aria-label={copiedAll ? 'Copied all warnings' : 'Copy all warnings'}
              title={copiedAll ? 'Copied' : 'Copy all warnings'}
            >
              <FaIcon icon={copiedAll ? faCheck : faCopy} class="h-3 w-3" />
              <span>{copiedAll ? 'Copied' : 'Copy all'}</span>
            </button>
          {/if}
          <button
            type="button"
            class="rounded-full border border-surface-700/70 bg-surface-900/70 p-1.5 text-surface-200 transition hover:border-surface-500 hover:text-white focus-visible:outline focus-visible:outline-2 focus-visible:outline-amber-300"
            onclick={onClose}
            aria-label="Close warnings panel"
            title="Close warnings panel"
          >
            <FaIcon icon={faXmark} class="h-3 w-3" />
          </button>
        </div>
      </div>
      {#if hasWarnings}
        <ul class="mt-3 max-h-[60vh] max-h-[60svh] max-h-[60dvh] space-y-2 overflow-y-auto pr-1 text-amber-50">
          {#each warnings as warning, index (warning.message + (warning.nodeId ?? '') + (warning.port ?? '') + index)}
            {@const warningKey = `${warning.message}-${warning.nodeId ?? ''}-${warning.port ?? ''}-${index}`}
            <li>
              <div class="flex gap-2 rounded border border-amber-700/40 bg-amber-950/40 px-3 py-2 text-left text-[0.85rem] leading-snug transition hover:border-amber-500/70 hover:bg-amber-900/50">
                <button
                  type="button"
                  class="flex min-w-0 flex-1 flex-col text-left transition hover:text-amber-50 focus-visible:outline focus-visible:outline-2 focus-visible:outline-amber-300 disabled:cursor-default disabled:opacity-60"
                  onclick={() => onFocus(warning)}
                  disabled={!warning.nodeId}
                >
                  <div class="flex items-center gap-2 text-[0.78rem] font-semibold text-amber-100">
                    {#if warning.nodeId}
                      <span>Node {warning.nodeId.slice(0, 8)}</span>
                    {:else}
                      <span>Graph</span>
                    {/if}
                    {#if warning.port}
                      <span class="text-amber-100/90">· {warning.port}</span>
                    {/if}
                    {#if warning.nodeId}
                      <span class="ml-auto rounded border border-amber-600/60 bg-amber-800/50 px-2 py-[1px] text-micro-tight uppercase tracking-[0.18em]">
                        Focus
                      </span>
                    {/if}
                  </div>
                  <div class="text-amber-100/90">{warning.message}</div>
                </button>
                <button
                  type="button"
                  class="mt-0.5 flex h-8 w-8 shrink-0 items-center justify-center rounded border border-amber-700/50 bg-amber-950/60 text-amber-100 transition hover:border-amber-400/80 hover:text-white focus-visible:outline focus-visible:outline-2 focus-visible:outline-amber-300"
                  onclick={() => copyWarning(warning, warningKey)}
                  aria-label={copiedWarningKey === warningKey ? 'Copied warning' : 'Copy warning'}
                  title={copiedWarningKey === warningKey ? 'Copied' : 'Copy warning'}
                >
                  <FaIcon icon={copiedWarningKey === warningKey ? faCheck : faClipboard} class="h-3.5 w-3.5" />
                </button>
              </div>
            </li>
          {/each}
        </ul>
      {/if}
    </div>
  </div>
{/if}
