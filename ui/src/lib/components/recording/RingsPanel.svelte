<script lang="ts">
  // Capture last: each camera's ring of its most recent frames. Capture writes
  // the ring's last seconds out as a new stored recording.
  import Icon from "#lib/components/common/Icon.svelte";
  import { identity } from "#lib/core/identity.svelte.js";
  import Badge from "#lib/kit/Badge.svelte";
  import Meter from "#lib/kit/Meter.svelte";
  import { cluster } from "#lib/stores/cluster.svelte.js";
  import { recordings } from "#lib/stores/recordings.svelte.js";
  import { durationText, dropSummary, ringSummary, sizeText } from "#lib/recording/format.js";

  const rings = $derived(recordings.listing?.rings ?? []);
  const clip = $derived(recordings.lastClip);
  const cameraName = (id: string) => identity.name(id, cluster.camera(id)?.name ?? id);
</script>

{#if rings.length === 0}
  <p class="none">No camera keeps a capture-last ring. Turn one on in Settings, Recording, to keep the last seconds of its frames ready to write out.</p>
{:else}
  <ul class="rings">
    {#each rings as ring (ring.camera)}
      {@const s = ringSummary(ring)}
      <li class="ring">
        <div class="top">
          <span class="cam">{cameraName(ring.camera)}</span>
          <Badge tone={s.tone} text={s.text} />
          <span class="grow"></span>
          <button type="button" class="btn" disabled={ring.state === "failed" || recordings.busy === "capture"} onclick={() => recordings.captureLast(ring.camera)}>
            <Icon name="history" size={13} />{recordings.busy === "capture" ? "Writing…" : `Capture last ${ring.seconds} s`}
          </button>
        </div>
        <Meter value={s.filled} color={s.filled >= 1 ? "var(--ok)" : "var(--info)"} />
        <span class="meta mono">{sizeText(ring.bytes)} of {sizeText(ring.budget_bytes)} · {ring.format ?? "no frames yet"}</span>
      </li>
    {/each}
  </ul>
{/if}

{#if clip}
  {@const drops = dropSummary(clip.frames_written, clip.frames_dropped)}
  <p class="clip"><Icon name="circle-check" size={13} /><span>Wrote <span class="mono">{clip.file}</span>: {durationText(clip.duration_ms)} from {cameraName(clip.camera)}, <span class="dropped t-{drops.tone}">{drops.text.toLowerCase()}</span></span></p>
{/if}

<style>
  .rings {
    display: flex;
    flex-direction: column;
    gap: 10px;
    margin: 0;
    padding: 2px 10px 6px;
    list-style: none;
  }
  .ring {
    display: flex;
    flex-direction: column;
    gap: 5px;
  }
  .top {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
  }
  .cam {
    font-size: 12.5px;
    font-weight: 600;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    min-width: 0;
  }
  .grow {
    flex: 1;
  }
  .meta {
    font-size: 10.5px;
    color: var(--fg-3);
  }
  .mono {
    font-family: var(--font-code);
    font-size: 11px;
  }
  .none {
    margin: 0;
    padding: 2px 10px 10px;
    font-size: 11.5px;
    color: var(--fg-3);
    line-height: 1.5;
  }
  .clip {
    display: flex;
    align-items: flex-start;
    gap: 6px;
    margin: 0;
    padding: 0 10px 10px;
    font-size: 11.5px;
    color: var(--fg-2);
  }
  .clip :global(.icon) {
    margin-top: 2px;
    color: var(--ok);
  }
  .dropped.t-ok {
    color: var(--ok);
  }
  .dropped.t-warn {
    color: var(--warn);
  }
  .dropped.t-err {
    color: var(--err);
  }
  .btn {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: 24px;
    padding: 0 10px;
    font-size: 11.5px;
    font-weight: 600;
    color: var(--fg);
    border: 1px solid var(--line-strong);
    border-radius: var(--r-2);
    white-space: nowrap;
  }
  .btn:hover:not(:disabled) {
    background: var(--s3);
  }
  .btn:disabled {
    opacity: 0.5;
    cursor: default;
  }
</style>
