<script lang="ts">
  // First run: team number, the cameras found and what each should do, and
  // how much of HeliOS to show to begin with. Takes under a minute; everything
  // can be changed later.
  import Icon from "#lib/components/common/Icon.svelte";
  import { guide, type Job } from "#lib/core/guide.svelte.js";
  import { colorVar, ID_COLORS, identity } from "#lib/core/identity.svelte.js";
  import { shell } from "#lib/core/shell.svelte.js";
  import { team } from "#lib/core/team.svelte.js";
  import { workspaces } from "#lib/core/workspace.svelte.js";
  import SecureForm from "#lib/components/security/SecureForm.svelte";
  import { auth } from "#lib/stores/auth.svelte.js";
  import { cluster } from "#lib/stores/cluster.svelte.js";
  import Feed from "#lib/vision/Feed.svelte";
  import { DEFAULT_OVERLAYS } from "#lib/vision/overlays.js";
  import { help } from "./help.svelte";

  let step = $state(0);
  let simple = $state(true);
  let teamText = $state(team.number ? String(team.number) : "");
  const STEPS = ["Team", "Cameras", "Screens", "Security", "Done"];
  const SECURITY = 3;
  const LAST = STEPS.length - 1;
  // FRC robots run open; anything else is offered a password first. The
  // choice is explicit either way, and the top bar always shows the result.
  let secureChoice = $state<"open" | "secure" | null>(null);
  const choice = $derived(secureChoice ?? (Number(teamText) > 0 ? "open" : "secure"));
  const blocked = $derived(step === SECURITY && choice === "secure" && auth.open);
  const overlays = { ...DEFAULT_OVERLAYS, histogram: false, hud: false };

  function finish(tour: boolean) {
    const n = Number(teamText);
    team.number = Number.isInteger(n) && n > 0 && n < 100000 ? n : null;
    // The names shown here were confirmed, so they count as chosen.
    for (const c of cluster.cameras) if (!identity.get(c.resourceId).name) identity.set(c.resourceId, { name: c.name });
    workspaces.setVisible(simple ? ["start", "settings"] : null);
    shell.go(simple ? "start" : "overview");
    help.finishOnboarding();
    if (tour) setTimeout(() => help.startTour(), 300);
  }

  function cycleColor(id: string) {
    const c = identity.get(id).color;
    identity.set(id, { color: ID_COLORS[c % ID_COLORS.length] });
  }
</script>

{#if help.onboarding}
  <div class="scrim"></div>
  <div class="dlg" role="dialog" aria-label="Set up HeliOS">
    <div class="steps">
      {#each STEPS as s, i (s)}
        <span class:on={i === step} class:done={i < step}><i>{#if i < step}<Icon name="check" size={10} stroke={3} />{:else}{i + 1}{/if}</i>{s}</span>
      {/each}
      <span class="grow"></span>
      <button type="button" class="skip" onclick={() => finish(false)}>Skip setup</button>
    </div>

    <div class="body">
      {#if step === 0}
        <h2>Welcome to HeliOS</h2>
        <p>HeliOS runs the robot's cameras and vision: it finds AprilTags and other targets, works out where the robot is, and sends the results to robot code. This sets the basics; everything can be changed later.</p>
        <label class="field">
          <span>Team number</span>
          <input inputmode="numeric" bind:value={teamText} placeholder="e.g. 1234" />
          <em>Robot code is expected at <code>{Number(teamText) > 0 ? `10.${Math.floor(Number(teamText) / 100)}.${Number(teamText) % 100}.2` : "10.TE.AM.2"}</code></em>
        </label>
        <div class="found">
          <Icon name="circle-check" size={14} />
          Found {cluster.nodes.length} devices and {cluster.cameras.length} cameras on this robot.
        </div>
      {:else if step === 1}
        <h2>Your cameras</h2>
        <p>Name each one so you can tell them apart, and choose what it does. The colour follows it everywhere.</p>
        <div class="cams">
          {#each cluster.cameras as c (c.resourceId)}
            <div class="cam" style:--c={colorVar(identity.get(c.resourceId).color)}>
              <div class="thumb"><Feed camera={c} {overlays} compact /></div>
              <button type="button" class="sw" onclick={() => cycleColor(c.resourceId)} aria-label="Change colour" data-tip="Change colour"></button>
              <input class="name" value={identity.name(c.resourceId, c.name)} aria-label="Camera name" onchange={(e) => identity.set(c.resourceId, { name: (e.currentTarget as HTMLInputElement).value.trim() || undefined })} />
              <span class="where">{c.mount}</span>
              {#if c.foreign}
                <span class="job ro">Run by {c.foreign}</span>
              {:else}
                <select class="job" value={guide.job(c)} aria-label="What this camera does" onchange={(e) => guide.setJob(c, (e.currentTarget as HTMLSelectElement).value as Job)}>
                  <option value="apriltag">Find AprilTags</option>
                  <option value="aruco">Find ArUco markers</option>
                  <option value="view">Video only</option>
                </select>
              {/if}
            </div>
          {/each}
        </div>
      {:else if step === 2}
        <h2>How much to show</h2>
        <p>You can add or hide screens any time from the + in the left rail.</p>
        <div class="choices">
          <button type="button" class="choice" class:on={simple} onclick={() => (simple = true)}>
            <Icon name="home" size={20} />
            <b>Start simple</b>
            <span>One Start screen with every camera, the setup checklist and any problems. Add Cameras, Pipelines, Hardware and more when you need them.</span>
            <em>Recommended for new teams</em>
          </button>
          <button type="button" class="choice" class:on={!simple} onclick={() => (simple = false)}>
            <Icon name="layout-grid" size={20} />
            <b>Everything</b>
            <span>All screens in the rail from the start: cameras, pipelines and graphs, robot geometry, calibration, hardware and processes.</span>
            <em>For teams who know what they want</em>
          </button>
        </div>
      {:else if step === SECURITY}
        <h2>Who can change this device</h2>
        {#if auth.secured}
          <div class="found"><Icon name="lock" size={14} />This device is secured with a password. Manage it in Settings, Security.</div>
        {:else}
          <p>Open devices need no login: anyone on the robot network can change pipelines, install updates and reboot. That is normal on an FRC robot. Anywhere else, secure it with a password.</p>
          <div class="choices">
            <button type="button" class="choice" class:on={choice === "open"} onclick={() => (secureChoice = "open")}>
              <Icon name="lock-open" size={20} />
              <b>Leave it open</b>
              <span>No password and no tokens. The top bar shows <b>Open</b> as a reminder. You can secure it any time in Settings.</span>
              <em>FRC robots on the field</em>
            </button>
            <button type="button" class="choice" class:on={choice === "secure"} onclick={() => (secureChoice = "secure")}>
              <Icon name="lock" size={20} />
              <b>Secure this device</b>
              <span>One device password for people using HeliOS; API tokens for tools like Atlas. Turn it off again in Settings.</span>
              <em>Labs, classrooms, shared networks</em>
            </button>
          </div>
          {#if choice === "secure"}
            <div class="secure-form"><SecureForm ondone={() => step++} /></div>
          {/if}
        {/if}
      {:else}
        <h2>Ready</h2>
        <p>
          {#if auth.secured}This device is <b>secured</b>: HeliOS asks for the password in new browsers.{:else}This device is <b>open</b>; the top bar says so, and Settings can secure it.{/if}
        </p>
        <p>Two things are still needed before matches, and the <b>Setup checklist</b> keeps track of them:</p>
        <ul>
          <li><b>Calibrate each camera</b> with a printed board (about two minutes each).</li>
          <li><b>Set where each camera is</b> on the robot (numbers, or click it on your CAD).</li>
        </ul>
        <p>A one-minute tour shows where everything is.</p>
      {/if}
    </div>

    <div class="foot">
      {#if step > 0}<button type="button" onclick={() => step--}>Back</button>{/if}
      <span class="grow"></span>
      {#if step < LAST}
        <button type="button" class="primary" disabled={blocked} data-tip={blocked ? "Set a password above, or choose Leave it open" : undefined} onclick={() => step++}>Next<Icon name="arrow-right" size={13} /></button>
      {:else}
        <button type="button" onclick={() => finish(false)}>Finish</button>
        <button type="button" class="primary" onclick={() => finish(true)}><Icon name="rocket" size={13} />Take the tour</button>
      {/if}
    </div>
  </div>
{/if}

<style>
  .scrim {
    position: fixed;
    inset: 0;
    z-index: 960;
    background: rgba(0, 0, 0, 0.55);
  }
  .dlg {
    position: fixed;
    z-index: 961;
    top: 50%;
    left: 50%;
    transform: translate(-50%, -50%);
    width: min(820px, calc(100vw - 40px));
    max-height: 88vh;
    display: flex;
    flex-direction: column;
    background: var(--s1);
    border: 1px solid var(--line-strong);
    border-radius: var(--r-3);
    box-shadow: var(--shadow);
  }
  .steps {
    display: flex;
    align-items: center;
    gap: 14px;
    padding: 10px 14px;
    border-bottom: 1px solid var(--line);
    font-size: 12px;
    color: var(--fg-3);
  }
  .steps span {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .steps i {
    display: grid;
    place-items: center;
    width: 18px;
    height: 18px;
    font-style: normal;
    font-size: 10.5px;
    font-weight: 700;
    border: 1px solid var(--line-strong);
    border-radius: var(--r-1);
  }
  .steps .on {
    color: var(--fg);
    font-weight: 600;
  }
  .steps .on i {
    color: var(--on-accent);
    background: var(--accent);
    border-color: var(--accent);
  }
  .steps .done i {
    color: var(--s0);
    background: var(--ok);
    border-color: var(--ok);
  }
  .grow {
    flex: 1 !important;
  }
  .skip {
    font-size: 12px;
    color: var(--fg-3);
  }
  .body {
    padding: 18px 22px;
    overflow-y: auto;
  }
  h2 {
    margin: 0 0 6px;
    font-size: 18px;
  }
  p {
    margin: 0 0 14px;
    font-size: 13px;
    line-height: 1.55;
    color: var(--fg-2);
  }
  .field {
    display: flex;
    flex-direction: column;
    gap: 5px;
    max-width: 320px;
    font-size: 12px;
    color: var(--fg-2);
  }
  .field input {
    height: 34px;
    padding: 0 10px;
    font: inherit;
    font-size: 15px;
    font-family: var(--font-code);
    color: var(--fg);
    background: var(--inset);
    border: 1px solid var(--line-strong);
    border-radius: var(--r-2);
    outline: none;
  }
  .field input:focus {
    border-color: var(--accent);
  }
  .field em {
    font-style: normal;
    font-size: 11.5px;
    color: var(--fg-3);
  }
  code {
    font-family: var(--font-code);
  }
  .found {
    display: flex;
    align-items: center;
    gap: 7px;
    margin-top: 16px;
    font-size: 12.5px;
    color: var(--ok);
  }
  .cams {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .cam {
    display: grid;
    grid-template-columns: 96px 14px 1fr 1fr 170px;
    align-items: center;
    gap: 10px;
    padding: 4px;
    border: 1px solid var(--line);
    border-left: 3px solid var(--c);
    border-radius: var(--r-2);
  }
  .thumb {
    display: flex;
    height: 56px;
    border-radius: var(--r-1);
    overflow: hidden;
  }
  .sw {
    width: 14px;
    height: 14px;
    border-radius: 50%;
    background: var(--c);
  }
  .name {
    height: 30px;
    padding: 0 8px;
    font: inherit;
    font-size: 13px;
    font-weight: 600;
    color: var(--fg);
    background: var(--inset);
    border: 1px solid var(--line);
    border-radius: var(--r-1);
    outline: none;
  }
  .name:focus {
    border-color: var(--accent);
  }
  .where {
    font-size: 11.5px;
    color: var(--fg-3);
  }
  .job {
    height: 30px;
    padding: 0 8px;
    font: inherit;
    font-size: 12.5px;
    color: var(--fg);
    background: var(--inset);
    border: 1px solid var(--line-strong);
    border-radius: var(--r-1);
  }
  .job.ro {
    display: flex;
    align-items: center;
    color: var(--fg-3);
    border-style: dashed;
  }
  .choices {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 12px;
  }
  .choice {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 6px;
    padding: 14px;
    text-align: left;
    border: 1px solid var(--line-strong);
    border-radius: var(--r-2);
    color: var(--fg-2);
  }
  .choice b {
    font-size: 14.5px;
    color: var(--fg);
  }
  .choice span {
    font-size: 12.5px;
    line-height: 1.5;
  }
  .choice em {
    font-style: normal;
    font-size: 11.5px;
    color: var(--fg-3);
  }
  .choice.on {
    border-color: var(--accent);
    box-shadow: 0 0 0 2px var(--accent-tint-strong);
  }
  .choice.on :global(svg) {
    color: var(--accent);
  }
  ul {
    margin: 0 0 14px;
    padding-left: 18px;
    font-size: 13px;
    line-height: 1.8;
    color: var(--fg-2);
  }
  .foot {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 10px 14px;
    border-top: 1px solid var(--line);
  }
  .foot button {
    display: flex;
    align-items: center;
    gap: 6px;
    height: 30px;
    padding: 0 14px;
    font-size: 12.5px;
    font-weight: 600;
    color: var(--fg);
    border: 1px solid var(--line-strong);
    border-radius: var(--r-1);
  }
  .foot .primary {
    color: var(--on-accent);
    background: var(--accent);
    border-color: var(--accent);
  }
  .foot .primary:disabled {
    opacity: 0.5;
  }
  .secure-form {
    margin-top: 14px;
  }
</style>
