import type { PageLoad } from './$types';

export const load: PageLoad = ({ params }) => {
  const streamId = decodeURIComponent(params.cameraId);
  return { streamId };
};

export const ssr = false;
