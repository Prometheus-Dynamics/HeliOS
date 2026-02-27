import { json } from '@sveltejs/kit';
import type { RequestHandler } from '@sveltejs/kit';
import payload from './payload.json';

export const GET: RequestHandler = async () => {
  return json(payload);
};
