import { buildHttpCandidateUrls } from '$lib/api/httpCandidates';
import { apiUrl } from '$lib/api/client';
import { fetchPeerStreamFormat } from '$lib/api/peers';
import { resolveStreamPreviewFormat } from '$lib/api/streamPreviewFormat';
import { StreamsApi } from '$lib/api/streamsApi';

import type { StreamPreviewControllerDeps } from './streamPreviewController';

export const STREAM_PREVIEW_CONTROLLER_BROWSER_DEPS: StreamPreviewControllerDeps = {
  apiUrl,
  buildHttpCandidateUrls,
  clearTimeout: globalThis.clearTimeout.bind(globalThis),
  fetchPeerStreamFormat,
  fetchStreamFormat: StreamsApi.streamFormat,
  now: () => Date.now(),
  resolveStreamPreviewFormat,
  setTimeout: globalThis.setTimeout.bind(globalThis)
};
