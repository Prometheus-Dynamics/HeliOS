<script lang="ts">
  type PipelinePluginProjectModalProps = {
    open: boolean;
    projectName?: string;
    projectLanguage?: string;
    error?: string | null;
    busy?: boolean;
    onClose?: () => void;
    onCreate?: () => void;
  };

  let {
    open,
    projectName = $bindable(''),
    projectLanguage = $bindable('rust'),
    error = null,
    busy = false,
    onClose = () => {},
    onCreate = () => {}
  }: PipelinePluginProjectModalProps = $props();

  export type $$Props = PipelinePluginProjectModalProps;
</script>

{#if open}
  <div class="fixed inset-0 z-40 bg-black/60 backdrop-blur-sm"></div>
  <div class="fixed left-1/2 top-24 z-50 w-full max-w-xl -translate-x-1/2 rounded border border-surface-700 bg-surface-950/95 p-6 shadow-2xl">
    <div class="flex items-center justify-between gap-3">
      <div>
        <p class="text-xs uppercase tracking-[0.3em] text-surface-500">New plugin project</p>
        <h2 class="text-lg font-semibold text-white">Create a Daedalus plugin</h2>
      </div>
      <button class="btn btn-3xs preset-outline uppercase tracking-[0.3em]" type="button" onclick={onClose}>
        Close
      </button>
    </div>
    <div class="mt-4 space-y-4">
      <label class="flex flex-col gap-1 text-sm">
        <span class="text-xs uppercase tracking-[0.3em] text-surface-500">Project name</span>
        <input class="input h-9" placeholder="example_plugin" bind:value={projectName} />
      </label>
      <label class="flex flex-col gap-1 text-sm">
        <span class="text-xs uppercase tracking-[0.3em] text-surface-500">Template</span>
        <select class="input h-9" bind:value={projectLanguage}>
          <option value="rust">Rust</option>
          <option value="python">Python</option>
          <option value="java">Java</option>
          <option value="c_cpp">C/C++</option>
        </select>
      </label>
      <p class="text-xs text-surface-500">Uses the Daedalus FFI examples as a starting point.</p>
      {#if error}
        <p class="text-xs text-error-300">{error}</p>
      {/if}
    </div>
    <div class="mt-6 flex items-center justify-end gap-3">
      <button class="btn btn-2xs preset-outline uppercase tracking-[0.3em]" type="button" onclick={onClose} disabled={busy}>
        Cancel
      </button>
      <button class="btn btn-2xs preset-filled-primary-500 uppercase tracking-[0.3em]" type="button" onclick={onCreate} disabled={busy}>
        {busy ? 'Creating…' : 'Create'}
      </button>
    </div>
  </div>
{/if}
