<script lang="ts">
  type PipelineIdePanelProps = {
    ideWorkspaceDir: string;
    ideEnabled: boolean;
    ideIframeUrl: string;
    onOpenIde?: () => void;
  };

  let {
    ideWorkspaceDir,
    ideEnabled,
    ideIframeUrl,
    onOpenIde = () => {}
  }: PipelineIdePanelProps = $props();

  export type $$Props = PipelineIdePanelProps;
</script>

<section class="flex min-h-0 flex-1 flex-col gap-6">
  <div class="grid gap-4">
    <div class="rounded border border-surface-800/60 bg-surface-950/60 shadow-lg shadow-black/30">
      <div class="flex items-center justify-between border-b border-surface-800/60 px-4 py-3">
        <div>
          <p class="text-xs uppercase tracking-[0.3em] text-surface-500">Workspace</p>
          <p class="text-sm font-semibold text-white">{ideWorkspaceDir}</p>
        </div>
        <button class="btn btn-2xs preset-outline uppercase tracking-[0.3em]" type="button" onclick={onOpenIde} disabled={!ideEnabled}>
          Open SDK tab
        </button>
      </div>
      <div class="relative aspect-[16/9] w-full overflow-hidden rounded-b">
        {#if ideEnabled}
          <iframe src={ideIframeUrl} class="h-full w-full border-0" title="Helios OpenVSCode"></iframe>
        {:else}
          <div class="flex h-full items-center justify-center bg-surface-900/70 text-sm text-amber-200">
            Enable the IDE in the preset to embed it here.
          </div>
        {/if}
      </div>
    </div>
  </div>
</section>
