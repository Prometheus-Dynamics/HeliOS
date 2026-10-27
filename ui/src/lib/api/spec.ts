// A pipeline's spec as the API takes it on a PUT. A PUT replaces the whole spec: a setting left out
// of it is cleared on the device. So every change starts from the pipeline's whole current spec and
// changes only its own field (`{ ...specOf(p), dictionary }`).

import type { Pipeline, PipelineSpec } from "./types.js";

/** The whole spec of pipeline `p` as it is now (its graph must be known). */
export function specOf(p: Pipeline): PipelineSpec {
  return {
    name: p.name,
    graph: p.graph as PipelineSpec["graph"],
    bindings: p.bindings,
    enabled: p.enabled,
    search_mode: p.search_mode ?? undefined,
    full_search_every: p.full_search_every ?? undefined,
    loss_full_search_after: p.loss_full_search_after ?? undefined,
    margin_growth_misses: p.margin_growth_misses ?? undefined,
    field_layout: p.field_layout ?? undefined,
    dictionary: p.dictionary ?? undefined,
    cpu: p.cpu ?? undefined,
    nice: p.nice ?? undefined,
    map: p.map,
  };
}
