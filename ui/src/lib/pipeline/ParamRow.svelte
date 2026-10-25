<script lang="ts">
  // One node parameter as a settings row: its readable label and unit, the right control, the
  // help on hover, and a reset when it is away from its default.
  import type { Param } from "#lib/api/catalog.js";
  import type { ParamValue } from "#lib/graph/graph.js";
  import ParamControl from "#lib/kit/ParamControl.svelte";
  import Prop from "#lib/kit/Prop.svelte";
  import { isChangedParam, labelOf } from "./params";

  let { param, value, onchange }: { param: Param; value: ParamValue | undefined; onchange: (v: ParamValue) => void } = $props();
  const shown = $derived(labelOf(param));
  const current = $derived(value ?? param.default);
</script>

<Prop label={shown.unit ? `${shown.label} (${shown.unit})` : shown.label} help="{param.help} ({param.name})" changed={isChangedParam(param, value)} onreset={() => onchange(param.default)}>
  <ParamControl {param} value={current} {onchange} />
</Prop>
