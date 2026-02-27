<script lang="ts">
  import type { Snippet } from 'svelte';
  import { Panel, StreamPreview } from '$lib';

  export type StreamPreviewItem = {
    id: string;
    name: string;
    status: string;
    captureSessionId?: string | null;
    captureSessionAlias?: string | null;
    cameraUid?: string | null;
    recordingActive?: boolean;
    recordingSinceMs?: number | null;
  };

type StreamsPanelProps = {
  streams?: StreamPreviewItem[];
  title?: string;
  eyebrow?: string;
  emptyMessage?: string;
  actions?: Snippet;
  children?: Snippet;
};

const {
  streams = [],
  title = 'Active streams',
  eyebrow = 'Stream snapshot',
  emptyMessage = 'No streams reported',
  actions,
  children
}: StreamsPanelProps = $props();

export type $$Props = StreamsPanelProps;
export interface $$Slots {
  default?: Record<string, never>;
  actions?: Record<string, never>;
}
</script>

<Panel tone="subtle" {eyebrow} {title} {actions}>
  {#if children}
    {@render children()}
  {:else}
    {#if streams.length === 0}
      <div class="rounded border border-dashed border-surface-700/70 bg-surface-950/40 px-3 py-4 text-xs uppercase tracking-[0.3em] text-surface-500">
        {emptyMessage}
      </div>
    {:else}
      <div class="grid gap-3 md:grid-cols-2 xl:grid-cols-3">
        {#each streams as stream (stream.id)}
          <div class="rounded border border-surface-800 bg-surface-950/50 p-2 shadow shadow-black/20">
            <StreamPreview
              name={stream.name}
              status={stream.status}
              recording={stream.recordingActive ?? false}
              captureSessionId={stream.captureSessionId}
              captureSessionAlias={stream.captureSessionAlias}
              cameraUid={stream.cameraUid}
            />
          </div>
        {/each}
      </div>
    {/if}
  {/if}
</Panel>
