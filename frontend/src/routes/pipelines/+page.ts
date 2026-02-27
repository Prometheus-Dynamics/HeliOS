import type { PageLoad } from './$types';
import { emptyPipelinePayload } from '$lib/api/pipelinesPayload';

export const load: PageLoad = () => {
  return emptyPipelinePayload();
};
