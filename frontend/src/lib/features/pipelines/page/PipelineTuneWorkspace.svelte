<script lang="ts">
  import type { Snippet } from 'svelte';
  import {
    createPipelineTuneWorkspaceContext,
    type PipelineTuneWorkspaceContext
  } from './pipelineTuneWorkspaceContext.svelte';
  import type { TuneDeps } from './pipelineTuneWorkspaceTypes';

  type TuneProps = Omit<TuneDeps, 'children'> & {
    children?: Snippet<[ { tune: PipelineTuneWorkspaceContext } ]>;
  };

  let props: TuneProps = $props();
  const getTune = createPipelineTuneWorkspaceContext(() => props);
</script>

{@render props.children?.({ tune: getTune() })}
