<script lang="ts">
  // The camera view's record control: record this camera, stop it while it runs
  // (with the elapsed time and size on the button), and capture the last seconds
  // when this camera keeps a ring.
  import type { Camera } from "#lib/api/model.js";
  import { identity } from "#lib/core/identity.svelte.js";
  import IconButton from "#lib/kit/IconButton.svelte";
  import { clockText, elapsedMs, sizeText } from "#lib/recording/format.js";
  import { recordings } from "#lib/stores/recordings.svelte.js";

  let { camera }: { camera: Camera } = $props();

  const id = $derived(camera.resourceId);
  const name = $derived(identity.name(id, camera.name));
  const running = $derived(recordings.activeFor(id));
  /** Another camera is recording: the recorder runs one at a time. */
  const other = $derived(recordings.active && !running ? recordings.active : null);
  const ring = $derived(recordings.ringFor(id));
  const elapsed = $derived(running ? elapsedMs(running, recordings.receivedAt, recordings.now) : 0);
</script>

{#if running}
  <IconButton
    icon="player-stop"
    label="Stop recording {name}"
    text="{clockText(elapsed)} · {sizeText(running.bytes)}"
    tone="danger"
    size={24}
    disabled={running.state === "finishing"}
    onclick={() => recordings.stop(running.id)}
  />
{:else}
  <IconButton
    icon="player-record"
    label={other ? "Another camera is recording" : `Record ${name}`}
    text="Record"
    size={24}
    disabled={other !== null || recordings.listing === null || recordings.busy === "start"}
    onclick={() => recordings.start(id)}
  />
{/if}
{#if ring}
  <IconButton
    icon="history"
    label="Capture the last {ring.seconds} seconds of {name}"
    text="Last {ring.seconds} s"
    size={24}
    disabled={ring.state === "failed" || recordings.busy === "capture"}
    onclick={() => recordings.captureLast(id)}
  />
{/if}
