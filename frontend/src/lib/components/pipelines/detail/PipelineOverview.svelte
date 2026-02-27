<script lang="ts">
  type StatusChipTone = 'default' | 'info' | 'success' | 'warning' | 'error';
  type AutosaveChipState = {
    value: string;
    tone: StatusChipTone;
    showRetry: boolean;
  };

  type Props = {
    autosaveChipState: AutosaveChipState;
    revisionChipValue: string;
    shortPipelineId: string;
    metaChipBase: string;
    metaChipTones: Record<StatusChipTone, string>;
    onRetrySave: () => void;
  };

  let {
    autosaveChipState,
    revisionChipValue,
    shortPipelineId,
    metaChipBase,
    metaChipTones,
    onRetrySave
  }: Props = $props();
</script>

<div class="flex flex-wrap items-center gap-2">
  <div class={`${metaChipBase} ${metaChipTones[autosaveChipState.tone]}`}>
    <span>Autosave</span>
    <span class="text-micro-tight font-semibold tracking-[0.2em]">{autosaveChipState.value}</span>
  </div>
  <div class={`${metaChipBase} ${metaChipTones.default}`}>
    <span>Revision</span>
    <span class="font-mono text-[0.62rem] font-semibold tracking-[0.18em] text-white">{revisionChipValue}</span>
  </div>
  <div class={`${metaChipBase} ${metaChipTones.default}`}>
    <span>ID</span>
    <span class="font-mono text-[0.62rem] font-semibold tracking-[0.18em] text-white">{shortPipelineId}</span>
  </div>
  {#if autosaveChipState.showRetry}
    <button
      type="button"
      class="text-micro-tight font-semibold uppercase tracking-[0.28em] text-error-200 underline decoration-dotted underline-offset-2 hover:text-error-100"
      onclick={onRetrySave}
    >
      Retry save
    </button>
  {/if}
</div>
