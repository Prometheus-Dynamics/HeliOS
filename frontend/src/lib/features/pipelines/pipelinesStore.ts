import { derived, get, writable } from 'svelte/store';
import type { PipelineOverviewPipeline } from '$lib/types/pipeline';

type PipelineMap = Record<string, PipelineOverviewPipeline>;

type PipelineUpdateFn = (pipeline: PipelineOverviewPipeline) => PipelineOverviewPipeline | null | undefined;

export type PipelinesStore = ReturnType<typeof createPipelinesStore>;

export function createPipelinesStore(initial: PipelineOverviewPipeline[] = []) {
  const initialOrder = uniqueIds(initial.map((pipeline) => pipeline.id));
  const entities = writable<PipelineMap>(mapFromList(initial));
  const order = writable<string[]>(initialOrder);

  const list = derived([order, entities], ([$order, $entities]) =>
    uniqueIds($order)
      .map((id) => $entities[id])
      .filter((pipeline): pipeline is PipelineOverviewPipeline => Boolean(pipeline))
  );

  function setAll(pipelines: PipelineOverviewPipeline[]) {
    entities.set(mapFromList(pipelines));
    order.set(uniqueIds(pipelines.map((pipeline) => pipeline.id)));
  }

  function upsert(pipeline: PipelineOverviewPipeline, position: 'end' | 'start' = 'end') {
    entities.update((current) => ({ ...current, [pipeline.id]: pipeline }));
    order.update((ids) => {
      if (ids.includes(pipeline.id)) {
        return ids;
      }
      return position === 'start' ? [pipeline.id, ...ids] : [...ids, pipeline.id];
    });
  }

  function update(id: string, updater: PipelineUpdateFn) {
    entities.update((current) => {
      const pipeline = current[id];
      if (!pipeline) return current;
      const next = updater(pipeline);
      if (!next) return current;
      if (next === pipeline) return current;
      return {
        ...current,
        [id]: next
      };
    });
  }

  function map(updater: PipelineUpdateFn) {
    entities.update((current) => {
      let changed = false;
      const next: PipelineMap = {};
      for (const [id, pipeline] of Object.entries(current)) {
        const updated = updater(pipeline) ?? pipeline;
        next[id] = updated;
        if (updated !== pipeline) {
          changed = true;
        }
      }
      return changed ? next : current;
    });
  }

  function remove(id: string) {
    entities.update((current) => {
      if (!current[id]) {
        return current;
      }
      const next = { ...current };
      delete next[id];
      return next;
    });
    order.update((ids) => ids.filter((entry) => entry !== id));
  }

  function snapshot(): PipelineMap {
    return get(entities);
  }

  return {
    list,
    entities,
    order,
    setAll,
    upsert,
    update,
    remove,
    map,
    snapshot
  };
}

function mapFromList(pipelines: PipelineOverviewPipeline[]): PipelineMap {
  return Object.fromEntries(pipelines.map((pipeline) => [pipeline.id, pipeline]));
}

function uniqueIds(ids: string[]): string[] {
  const out: string[] = [];
  const seen = new Set<string>();
  for (const id of ids) {
    if (seen.has(id)) continue;
    seen.add(id);
    out.push(id);
  }
  return out;
}
