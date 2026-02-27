import { extractError, extractMessage, summarizeErrorBody } from '$lib/api/errors';
import { apiFetch } from '$lib/api/apiFetch';

export const REQUESTED_BY = 'deck-ui';

export { extractError, extractMessage, summarizeErrorBody };
export { apiFetch };
