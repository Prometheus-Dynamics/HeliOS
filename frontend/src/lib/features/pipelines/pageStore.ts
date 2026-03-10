import { writable } from 'svelte/store';
import { createPipelineUpdatesStore } from './updatesStore';
import { createPipelineController } from './controller';
import type { PipelinePagePayload } from '$lib/types/pipeline';

export type PipelineActiveTab = 'pipeline' | 'tune';

export function createPipelinePageStore(initial: PipelinePagePayload) {
  const activeTab = writable<PipelineActiveTab>('pipeline');
  const registryDrawerOpen = writable(false);
  const pipelineUpdates = createPipelineUpdatesStore();
  const controller = createPipelineController(initial, { saveOverride: pipelineUpdates.saveOverride });
  const unsubscribe = controller.stores.pipeline.selectedPipelineId.subscribe((pipelineId) => {
    pipelineUpdates.connectPipeline(pipelineId ?? null);
  });

  return {
    activeTab,
    registryDrawerOpen,
    pipelineUpdates,
    controller,
    dispose: () => {
      unsubscribe();
      pipelineUpdates.destroy();
      controller.actions.dispose();
    }
  };
}
