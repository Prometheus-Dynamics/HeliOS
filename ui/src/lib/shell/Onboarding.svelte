<script lang="ts">
  // First run: only what nothing else asks. The team number (unless the robot controller gives it)
  // and, on a device that has security, whether to leave it open. It then lands on Set up, whose
  // checklist carries the rest: names, what each camera runs, calibration, placement, the field.
  import Icon from "#lib/components/common/Icon.svelte";
  import { shell } from "#lib/core/shell.svelte.js";
  import { team } from "#lib/core/team.svelte.js";
  import { roborioAddress } from "#lib/core/robot-facts.js";
  import SecureForm from "#lib/components/security/SecureForm.svelte";
  import { auth } from "#lib/stores/auth.svelte.js";
  import { cluster } from "#lib/stores/cluster.svelte.js";
  import { device } from "#lib/stores/device.svelte.js";
  import { help } from "./help.svelte";

  let step = $state(0);
  let teamText = $state("");
  let overriding = $state(false);
  // A team the robot controller gives is shown, not asked again, unless the user overrides it.
  const fromController = $derived(team.source === "controller" && !overriding);
  const teamNumber = $derived(fromController ? (team.number ?? 0) : Number(teamText));
  $effect(() => {
    if (!teamText && team.number) teamText = String(team.number);
  });
  // The security step is the owner's: a guest's access belongs to its host, so it is not offered.
  const STEPS = $derived(device.has("security") ? ["Team", "Security", "Done"] : ["Team", "Done"]);
  const SECURITY = $derived(STEPS.indexOf("Security"));
  const LAST = $derived(STEPS.length - 1);
  // FRC robots run open; anything else is offered a password first. The
  // choice is explicit either way, and the top bar always shows the result.
  let secureChoice = $state<"open" | "secure" | null>(null);
  const choice = $derived(secureChoice ?? (teamNumber > 0 ? "open" : "secure"));
  const blocked = $derived(step === SECURITY && choice === "secure" && auth.open);

  function finish(tour: boolean) {
    // The controller's team is already the device's answer; only a team the user typed is saved.
    if (!fromController) {
      const n = Number(teamText);
      team.number = Number.isInteger(n) && n > 0 && n < 100000 ? n : null;
    }
    shell.go("setup");
    help.finishOnboarding();
    if (tour) setTimeout(() => help.startTour(), 300);
  }

  /** Out of the wizard without choosing anything: nothing (the team) counts as set up. */
  function skip() {
    shell.go("run");
    help.finishOnboarding();
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
      <button type="button" class="skip" onclick={skip}>Skip setup</button>
    </div>

    <div class="body">
      {#if step === 0}
        <h2>Welcome to HeliOS</h2>
        <p>HeliOS runs the robot's cameras and vision: it finds AprilTags and other targets, works out where the robot is, and sends the results to robot code. This asks for the team; the <b>Set up</b> screen walks through the rest, camera by camera.</p>
        {#if fromController}
          <div class="field">
            <span>Team number</span>
            <div class="from"><Icon name="circle-check" size={14} /> Team {team.number} from {team.controllerName ?? "the robot controller"}</div>
            <button type="button" class="link" onclick={() => { overriding = true; teamText = team.number ? String(team.number) : ""; }}>Use a different team</button>
          </div>
        {:else}
          <label class="field">
            <span>Team number</span>
            <input inputmode="numeric" bind:value={teamText} placeholder="e.g. 1234" />
            <em>Robot code is expected at <code>{roborioAddress(teamNumber) ?? "10.TE.AM.2"}</code></em>
          </label>
        {/if}
        <div class="found">
          <Icon name="circle-check" size={14} />
          Found {cluster.nodes.length} {cluster.nodes.length === 1 ? "device" : "devices"} and {cluster.cameras.length} {cluster.cameras.length === 1 ? "camera" : "cameras"} on this robot.
        </div>
      {:else if step === SECURITY}
        <h2>Who can change this device</h2>
        {#if auth.secured}
          <div class="found"><Icon name="lock" size={14} />This device is secured with a password. Manage it under Device › Robot link.</div>
        {:else}
          <p>Open devices need no login: anyone on the robot network can change pipelines, install updates and reboot. That is normal on an FRC robot. Anywhere else, secure it with a password.</p>
          <div class="choices">
            <button type="button" class="choice" class:on={choice === "open"} onclick={() => (secureChoice = "open")}>
              <Icon name="lock-open" size={20} />
              <b>Leave it open</b>
              <span>No password and no tokens. The top bar shows <b>Open</b> as a reminder. You can secure it any time under Device › Robot link.</span>
              <em>FRC robots on the field</em>
            </button>
            <button type="button" class="choice" class:on={choice === "secure"} onclick={() => (secureChoice = "secure")}>
              <Icon name="lock" size={20} />
              <b>Secure this device</b>
              <span>One device password for people using HeliOS; API tokens for tools like Atlas. Turn it off again under Device › Robot link.</span>
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
          {#if auth.secured}This device is <b>secured</b>: HeliOS asks for the password in new browsers.{:else}This device is <b>open</b>; the top bar says so, and Device › Robot link can secure it.{/if}
        </p>
        <p>The <b>Set up</b> screen is next. Its checklist is each camera's status chain, so it says what is still missing and opens the pane that fixes it:</p>
        <ul>
          <li><b>Name each camera</b> and choose what it runs; tags show as soon as a pipeline does.</li>
          <li><b>Calibrate each camera</b> with a printed board (about two minutes each).</li>
          <li><b>Place each camera</b> on the robot (numbers, or click it on your CAD), and pick the field.</li>
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
        <button type="button" onclick={() => finish(false)}>Go to Set up</button>
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
    width: min(720px, calc(100vw - 24px));
    max-height: 88vh;
    display: flex;
    flex-direction: column;
    background: var(--s1);
    border: 1px solid var(--line-strong);
    border-radius: var(--r-3);
    box-shadow: var(--shadow);
  }
  .from {
    display: flex;
    align-items: center;
    gap: 6px;
    margin-top: 4px;
  }
  .link {
    align-self: flex-start;
    margin-top: 4px;
    padding: 0;
    border: 0;
    background: none;
    color: var(--accent, var(--text));
    font-size: 12px;
    text-decoration: underline;
    cursor: pointer;
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
  .choices {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(240px, 1fr));
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
