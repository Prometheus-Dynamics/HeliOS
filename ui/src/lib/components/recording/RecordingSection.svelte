<script lang="ts">
  // The recorder's settings: where recordings go, the free space kept, the frames
  // stored, the frame rate limit, capture-last, automatic recording and
  // NetworkTables control. Saved as the whole settings object; the device's
  // refusals stay on the form.
  import { untrack } from "svelte";
  import { identity } from "#lib/core/identity.svelte.js";
  import Num from "#lib/kit/Num.svelte";
  import Prop from "#lib/kit/Prop.svelte";
  import Seg from "#lib/kit/Seg.svelte";
  import Switch from "#lib/kit/Switch.svelte";
  import { cluster } from "#lib/stores/cluster.svelte.js";
  import { recordings } from "#lib/stores/recordings.svelte.js";
  import { CAPTURE_LAST_RANGE, MAX_FPS_RANGE, MIN_TOTAL_MIB, formFromSettings, isUnchanged, settingsFromForm, toggleInList, validateForm, type RecordingForm } from "#lib/recording/mapping.js";

  const DEFAULT_FORM: RecordingForm = { storageDir: "", reserveMiB: 2048, maxTotalMiB: null, frames: "raw", maxFps: 30, captureLastSeconds: 10, captureLastCameras: [], autoRecord: false, ntControl: true };
  const GIB = 1 << 30;

  /** The form being edited; it follows the device's settings until the first edit. */
  let form = $state<RecordingForm>(DEFAULT_FORM);
  let loaded = $state(false);
  const saved = $derived(recordings.settings ? formFromSettings(recordings.settings) : null);
  const errors = $derived(validateForm(form));
  /** The budget the folder has now (the saved settings', or the device's default), from the listing. */
  const budgetNow = $derived(recordings.listing?.storage?.max_total_bytes ?? null);
  const holdingNow = $derived(recordings.listing?.storage?.recordings_bytes ?? null);
  const invalid = $derived(Object.keys(errors).length > 0);
  const dirty = $derived(recordings.settings !== null && !isUnchanged(form, recordings.settings));
  const saving = $derived(recordings.busy === "save");

  $effect(() => {
    void recordings.loadSettings();
  });
  $effect(() => {
    const next = recordings.settings;
    if (next && !untrack(() => dirty)) {
      form = formFromSettings(next);
      loaded = true;
    }
  });

  function changed<K extends keyof RecordingForm>(key: K): boolean {
    return saved !== null && JSON.stringify(form[key]) !== JSON.stringify(saved[key]);
  }

  function reset<K extends keyof RecordingForm>(key: K) {
    if (saved) form[key] = saved[key];
  }

  async function save() {
    if (!recordings.settings || invalid || !dirty) return;
    await recordings.saveSettings(settingsFromForm(form, recordings.settings));
  }

  function revert() {
    if (recordings.settings) form = formFromSettings(recordings.settings);
  }

  const cameraName = (id: string) => identity.name(id, cluster.camera(id)?.name ?? id);
</script>

{#if !loaded}
  <p class="hint">{recordings.settingsError ? `The recording settings could not be read: ${recordings.settingsError}` : "Reading the recording settings..."}</p>
{:else}
  <Prop label="Folder" help="Where recordings are written. Empty: a mounted USB drive (under the USB roots), else the device's own folder." changed={changed("storageDir")} onreset={() => reset("storageDir")}>
    <input class="text mono" type="text" placeholder="Automatic" bind:value={form.storageDir} aria-label="Recordings folder" aria-invalid={errors.storageDir ? "true" : undefined} />
  </Prop>
  {#if errors.storageDir}<p class="field-err">{errors.storageDir}</p>{/if}

  <Prop label="Keep free" help="Free space the recorder always leaves on the disk. Recording stops, and is refused, below it." changed={changed("reserveMiB")} onreset={() => reset("reserveMiB")}>
    <span class="num">
      <Num bind:value={form.reserveMiB} min={0} max={1_000_000} step={1} width={90} unit="MiB" label="Free space kept in reserve, MiB" />
      <span class="small dim">{(form.reserveMiB / 1024).toFixed(1)} GiB</span>
    </span>
  </Prop>
  {#if errors.reserveMiB}<p class="field-err">{errors.reserveMiB}</p>{/if}

  <Prop label="Keep at most" help="The most the recordings folder holds. Before a new part starts, the oldest finished recordings are deleted to fit. The recording being written is never deleted." changed={changed("maxTotalMiB")} onreset={() => reset("maxTotalMiB")}>
    <span class="num">
      <Switch checked={form.maxTotalMiB !== null} label="Set a size limit for the recordings folder" onchange={(on) => (form.maxTotalMiB = on ? (saved?.maxTotalMiB ?? 1024) : null)} />
      {#if form.maxTotalMiB !== null}
        <Num value={form.maxTotalMiB} min={MIN_TOTAL_MIB} max={1_000_000} step={1} width={90} unit="MiB" label="Recordings folder size limit, MiB" onchange={(v) => (form.maxTotalMiB = v)} />
      {/if}
      {#if budgetNow !== null}
        <span class="small dim">{(budgetNow / GIB).toFixed(1)} GiB in effect{form.maxTotalMiB === null ? ", the device's default" : ""}{holdingNow !== null ? `, ${(holdingNow / GIB).toFixed(2)} GiB held` : ""}</span>
      {/if}
    </span>
  </Prop>
  {#if errors.maxTotalMiB}<p class="field-err">{errors.maxTotalMiB}</p>{/if}

  <Prop label="Frames" help="Raw keeps the sensor's 16-bit frames exactly. Luma keeps the image processor's 8-bit picture: about half the size." changed={changed("frames")} onreset={() => reset("frames")}>
    <Seg
      label="Frames stored"
      value={form.frames}
      options={[
        { value: "raw", label: "Raw" },
        { value: "luma", label: "Luma" },
      ]}
      onchange={(v) => (form.frames = v)}
    />
  </Prop>

  <Prop label="Frame rate limit" help="Caps the frames written per second. Off: every frame the camera delivers." changed={changed("maxFps")} onreset={() => reset("maxFps")}>
    <span class="num">
      <Switch checked={form.maxFps !== null} label="Limit the frame rate" onchange={(on) => (form.maxFps = on ? (saved?.maxFps ?? 30) : null)} />
      {#if form.maxFps !== null}
        <Num value={form.maxFps} min={MAX_FPS_RANGE.min} max={MAX_FPS_RANGE.max} step={1} width={70} unit="fps" label="Frame rate limit" onchange={(v) => (form.maxFps = v)} />
      {/if}
    </span>
  </Prop>
  {#if errors.maxFps}<p class="field-err">{errors.maxFps}</p>{/if}

  <Prop label="Capture last" help="How many seconds of frames each camera with a ring keeps ready to write out at once." changed={changed("captureLastSeconds")} onreset={() => reset("captureLastSeconds")}>
    <Num bind:value={form.captureLastSeconds} min={CAPTURE_LAST_RANGE.min} max={CAPTURE_LAST_RANGE.max} step={1} width={70} unit="s" label="Capture-last seconds" />
  </Prop>
  {#if errors.captureLastSeconds}<p class="field-err">{errors.captureLastSeconds}</p>{/if}

  <Prop label="Cameras with a ring" help="A ring holds the last seconds in memory. No camera ticked: no rings." stacked changed={changed("captureLastCameras")} onreset={() => reset("captureLastCameras")}>
    <div class="cams">
      {#each cluster.cameras as c (c.resourceId)}
        <label class="check">
          <input type="checkbox" checked={form.captureLastCameras.includes(c.resourceId)} onchange={() => (form.captureLastCameras = toggleInList(form.captureLastCameras, c.resourceId))} />
          <span>{cameraName(c.resourceId)}</span>
        </label>
      {:else}
        <span class="small dim">No cameras yet</span>
      {/each}
    </div>
  </Prop>

  <Prop label="Record automatically" help="One recording per enabled period of the robot, named after the match. Reads the field's control data over NetworkTables." changed={changed("autoRecord")} onreset={() => reset("autoRecord")}>
    <Switch bind:checked={form.autoRecord} label="Record automatically" />
  </Prop>

  <Prop label="Control over NetworkTables" help="Robot code can start and stop a recording, and capture the last seconds, over NetworkTables." changed={changed("ntControl")} onreset={() => reset("ntControl")}>
    <Switch bind:checked={form.ntControl} label="Control recording over NetworkTables" />
  </Prop>

  <div class="foot">
    {#if recordings.settingsError}<p class="server-err">{recordings.settingsError}</p>{/if}
    <span class="grow"></span>
    {#if dirty}<button type="button" class="mini" onclick={revert}>Revert</button>{/if}
    <button type="button" class="save" onclick={save} disabled={!dirty || invalid || saving}>{saving ? "Saving..." : "Save"}</button>
  </div>
{/if}

<style>
  .text {
    width: 220px;
    height: 24px;
    padding: 0 6px;
    border: 1px solid var(--line-strong);
    border-radius: var(--r-1);
    background: var(--s0);
    color: var(--fg);
    font-size: 12px;
  }
  .mono {
    font-family: var(--font-code);
    font-size: 11.5px;
  }
  .num {
    display: inline-flex;
    align-items: center;
    gap: 10px;
  }
  .cams {
    display: flex;
    flex-wrap: wrap;
    gap: 6px 14px;
  }
  .check {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    font-size: 12px;
    color: var(--fg-2);
  }
  .small {
    font-size: 11px;
  }
  .dim {
    color: var(--fg-3);
  }
  .hint {
    margin: 4px 10px 8px;
    font-size: 11.5px;
    color: var(--fg-3);
  }
  .field-err {
    margin: -2px 10px 6px;
    font-size: 11px;
    color: var(--err);
  }
  .foot {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px 10px 10px;
  }
  .grow {
    flex: 1;
  }
  .server-err {
    margin: 0;
    font-size: 11.5px;
    color: var(--err);
  }
  .mini {
    height: 24px;
    padding: 0 10px;
    font-size: 11.5px;
    font-weight: 600;
    color: var(--fg-2);
    border: 1px solid var(--line-strong);
    border-radius: var(--r-1);
  }
  .mini:hover {
    color: var(--fg);
    background: var(--s3);
  }
  .save {
    height: 26px;
    padding: 0 14px;
    font-size: 12px;
    font-weight: 600;
    color: var(--on-accent);
    background: var(--accent);
    border-radius: var(--r-2);
  }
  .save:disabled {
    opacity: 0.45;
    cursor: default;
  }
</style>
