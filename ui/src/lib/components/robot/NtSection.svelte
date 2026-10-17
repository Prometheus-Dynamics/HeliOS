<script lang="ts">
  // Where the robot's pipelines publish for robot code: NetworkTables (NT4)
  // on the team's roboRIO, or another server. Stored on the device; the
  // engine publishes every frame's outputs while connected.
  import { api, errorText } from "#lib/api/client.js";
  import type { Nt4Settings, Nt4State } from "#lib/api/types.js";
  import { team } from "#lib/core/team.svelte.js";
  import Badge from "#lib/kit/Badge.svelte";
  import Num from "#lib/kit/Num.svelte";
  import Prop from "#lib/kit/Prop.svelte";
  import Switch from "#lib/kit/Switch.svelte";
  import { toasts } from "#lib/stores/toasts.svelte.js";

  let nt = $state<Nt4State | null>(null);
  let error = $state<string | null>(null);

  async function load() {
    try {
      nt = await api.nt4();
      error = null;
      if (nt.settings.team) team.number = nt.settings.team;
    } catch (e) {
      error = errorText(e);
    }
  }

  async function save(change: Partial<Nt4Settings>) {
    if (!nt) return;
    const settings = { ...nt.settings, ...change };
    try {
      nt = await api.setNt4(settings);
      if (settings.team !== undefined) team.number = settings.team;
    } catch (e) {
      toasts.error(`NetworkTables: ${errorText(e)}`);
    }
  }

  $effect(() => {
    void load();
    // The connection state follows the engine; check it while this is open.
    const timer = window.setInterval(() => void load(), 3000);
    return () => clearInterval(timer);
  });

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

{#if error}
  <Prop label="NetworkTables"><span class="small dim">{error}</span></Prop>
{:else if nt}
  <Prop label="Team number" help="Robot code is on the team's roboRIO (10.TE.AM.2); pipelines publish there.">
    <Num value={nt.settings.team ?? 0} min={0} max={25599} width={90} label="Team number" onchange={(v) => save({ team: v || null })} />
  </Prop>
  <Prop label="NetworkTables" help="Every frame's pipeline outputs, latency and fps, for robot code.">
    <Switch checked={nt.settings.enabled} label="Publish to NetworkTables" onchange={(v) => save({ enabled: v })} />
    {#if status}<Badge tone={status.tone as "ok"} text={status.text} />{/if}
  </Prop>
  <Prop label="Server" help="Leave empty for the team's roboRIO; set a host for a simulator or another server.">
    <input class="text mono" type="text" placeholder={nt.settings.team ? `10.${Math.floor(nt.settings.team / 100)}.${nt.settings.team % 100}.2` : "roboRIO"} value={nt.settings.server ?? ""} onchange={(e) => save({ server: e.currentTarget.value.trim() || null })} aria-label="NetworkTables server" />
  </Prop>
  <Prop label="Topics" help="Each pipeline publishes <prefix>/<pipeline>/<output> (JSON), plus latency_ms, fps and capture_time_us.">
    <input class="text mono" type="text" value={nt.settings.prefix} onchange={(e) => save({ prefix: e.currentTarget.value.trim() || "/helios" })} aria-label="NetworkTables topic prefix" />
  </Prop>
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
