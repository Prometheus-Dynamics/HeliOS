<script lang="ts">
  // One stored recording: its facts, Download, Export for Eidos (with what it
  // wrote), and Delete behind a confirm step.
  import Icon from "#lib/components/common/Icon.svelte";
  import { identity } from "#lib/core/identity.svelte.js";
  import Badge from "#lib/kit/Badge.svelte";
  import type { StoredRecording } from "#lib/api/recording-types.js";
  import { cluster } from "#lib/stores/cluster.svelte.js";
  import { recordings } from "#lib/stores/recordings.svelte.js";
  import { dropSummary, durationText, eidosCameraText, sizeText, stopReasonText, whenText } from "#lib/recording/format.js";
  import { recordedAt } from "#lib/recording/mapping.js";

  let { item }: { item: StoredRecording } = $props();

  let confirming = $state(false);
  const summary = $derived(item.summary);
  const drops = $derived(summary ? dropSummary(summary.frames_written, summary.frames_dropped) : null);
  const exported = $derived(recordings.exports[item.id]);
  const link = $derived(recordings.downloadUrl(item.id));
  const busy = $derived(recordings.busy);
  const cameraName = (id: string) => identity.name(id, cluster.camera(id)?.name ?? id);
  const reason = $derived(summary?.stop_reason && summary.stop_reason !== "requested" ? stopReasonText(summary.stop_reason) : null);
</script>

<div class="item">
  <div class="srow">
    <span class="name">
      <span class="id mono" title={item.file}>{item.id}</span>
      <span class="sub">
        {#if summary}part {summary.index}{:else}unfinished{/if}{#if reason} · {reason}{/if}
      </span>
    </span>
    <span class="cam">{summary ? cameraName(summary.camera) : "-"}</span>
    <span class="mono dim">{whenText(recordedAt(item))}</span>
    <span class="r mono">{summary ? durationText(summary.duration_ms) : "-"}</span>
    <span class="r mono">{sizeText(item.bytes)}</span>
    <span>{#if drops}<Badge tone={drops.tone} text={drops.text} />{:else}<span class="dim">-</span>{/if}</span>
    <span class="acts">
      {#if confirming}
        <span class="ask">Delete it?</span>
        <button type="button" class="act danger" disabled={busy === `delete:${item.id}`} onclick={async () => { await recordings.remove(item.id); confirming = false; }}>Delete</button>
        <button type="button" class="act" onclick={() => (confirming = false)}>Keep</button>
      {:else}
        <button type="button" class="act" disabled={!summary || busy === `export:${item.id}`} onclick={() => recordings.exportEidos(item.id)} data-tip={summary ? "Write the replay input Eidos reads, beside this file" : "Only a finished part can be exported"}>
          {busy === `export:${item.id}` ? "Exporting…" : "Export for Eidos"}
        </button>
        {#if link}
          <a class="act" href={link} download={item.file}><Icon name="download" size={12} />Download</a>
        {:else}
          <span class="act dis" data-tip="Downloads come from the device">Download</span>
        {/if}
        <button type="button" class="act" aria-label="Delete {item.id}" onclick={() => (confirming = true)}><Icon name="trash" size={12} /></button>
      {/if}
    </span>
  </div>
  {#if exported}
    <div class="export">
      <Icon name="circle-check" size={13} />
      <div class="what">
        <span><b>Eidos input ready:</b> {exported.frames} frames at {exported.width}×{exported.height}, {eidosCameraText(exported.camera)}.</span>
        <span class="mono dir">{exported.dir}</span>
        <span class="mono files">{exported.files.join(" · ")}</span>
      </div>
    </div>
  {/if}
</div>

<style>
  .item {
    border-bottom: 1px solid var(--line);
  }
  .item:hover {
    background: var(--s2);
  }
  :global(.srow) {
    min-height: 40px;
  }
  .name {
    display: flex;
    flex-direction: column;
    gap: 1px;
    min-width: 0;
  }
  .id {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--fg);
  }
  .sub {
    font-size: 10.5px;
    color: var(--fg-3);
  }
  .cam {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .mono {
    font-family: var(--font-code);
    font-size: 11px;
  }
  .dim {
    color: var(--fg-3);
  }
  .r {
    text-align: right;
  }
  .acts {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: 4px;
    white-space: nowrap;
  }
  .act {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    height: 22px;
    padding: 0 8px;
    font-size: 11px;
    font-weight: 600;
    color: var(--fg-2);
    border: 1px solid var(--line-strong);
    border-radius: var(--r-1);
  }
  .act:hover:not(:disabled) {
    color: var(--fg);
    background: var(--s3);
  }
  .act:disabled,
  .act.dis {
    opacity: 0.45;
    cursor: default;
  }
  .act.danger {
    color: var(--on-accent);
    background: var(--err);
    border-color: transparent;
  }
  .ask {
    font-size: 11px;
    color: var(--fg-2);
    margin-right: 2px;
  }
  .export {
    display: flex;
    align-items: flex-start;
    gap: 8px;
    padding: 2px 10px 10px 10px;
    font-size: 11.5px;
    color: var(--fg-2);
  }
  .export :global(.icon) {
    margin-top: 2px;
    color: var(--ok);
  }
  .what {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }
  .dir,
  .files {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--fg-3);
  }
</style>
