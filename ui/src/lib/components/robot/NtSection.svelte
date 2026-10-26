<script lang="ts">
  // Where the robot's pipelines publish for robot code: NetworkTables (NT4) on the team's
  // roboRIO, or another server. The settings are kept on the device; the team and the server
  // show where they come from (set here, the robot controller, or nothing).
  import type { FactSource } from "#lib/api/types.js";
  import { team } from "#lib/core/team.svelte.js";
  import { roborioAddress } from "#lib/core/robot-facts.js";
  import Badge from "#lib/kit/Badge.svelte";
  import Num from "#lib/kit/Num.svelte";
  import Prop from "#lib/kit/Prop.svelte";
  import Switch from "#lib/kit/Switch.svelte";

  const nt = $derived(team.robot);

  $effect(() => {
    // The connection state follows the engine; check it while this is open.
    const timer = window.setInterval(() => void team.load(), 3000);
    return () => clearInterval(timer);
  });

  const SOURCE_TEXT: Record<FactSource, string> = { user: "Set here", controller: "From the robot controller", none: "Not set" };

  const status = $derived(
    !nt
      ? null
      : !nt.settings.enabled
        ? { tone: "neutral", text: "Off" }
        : !nt.server
          ? { tone: "warn", text: "Set a team number" }
          : nt.connected
            ? { tone: "ok", text: "Connected" }
            : { tone: "warn", text: `Waiting for ${nt.server}` },
  );
</script>

{#if nt}
  <Prop label="Team number" help="Robot code is on the team's roboRIO (10.TE.AM.2). Leave it empty to use the team the robot controller reports.">
    <Num value={nt.settings.team ?? 0} min={0} max={25599} width={90} label="Team number" onchange={(v) => team.save({ team: v || null })} />
    <Badge tone={team.source === "controller" ? "ok" : team.source === "user" ? "neutral" : "warn"} text={team.source === "controller" && team.controllerName ? `${team.number} from ${team.controllerName}` : SOURCE_TEXT[team.source]} />
  </Prop>
  <Prop label="NetworkTables" help="Every frame's pipeline outputs, latency and fps, for robot code.">
    <Switch checked={nt.settings.enabled} label="Publish to NetworkTables" onchange={(v) => team.save({ enabled: v })} />
    {#if status}<Badge tone={status.tone as "ok"} text={status.text} />{/if}
  </Prop>
  <Prop label="Robot code at" help="The server publishing goes to. Leave the server empty to use the team's roboRIO, or the robot controller's own server.">
    <span class="mono">{team.ntServer}</span>
    <Badge tone="neutral" text={SOURCE_TEXT[nt.effective.nt4_server.source]} />
  </Prop>
  <Prop label="Server" help="Set a host for a simulator or another server.">
    <input class="text mono" type="text" placeholder={roborioAddress(nt.settings.team ?? 0) ?? "roboRIO"} value={nt.settings.server ?? ""} onchange={(e) => team.save({ server: e.currentTarget.value.trim() || null })} aria-label="NetworkTables server" />
  </Prop>
  <Prop label="Topics" help="Each pipeline publishes <prefix>/<pipeline>/<output> (JSON), plus latency_ms, fps and capture_time_us.">
    <input class="text mono" type="text" value={nt.settings.prefix} onchange={(e) => team.save({ prefix: e.currentTarget.value.trim() || "/helios" })} aria-label="NetworkTables topic prefix" />
  </Prop>
{:else}
  <Prop label="NetworkTables"><span class="small dim">Loading the robot's settings…</span></Prop>
{/if}

<style>
  .text {
    width: 140px;
    height: 24px;
    padding: 0 6px;
    border: 1px solid var(--border);
    border-radius: 5px;
    background: var(--bg-1);
    color: var(--text);
    font-size: 12px;
  }
  .small {
    font-size: 11px;
  }
  .dim {
    color: var(--text-dim);
  }
</style>
