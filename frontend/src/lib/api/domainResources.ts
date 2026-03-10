import { subscribeDomainInvalidations, type DomainUpdateKind } from '$lib/api/invalidation';
import type { RealtimeUpdateEvent } from '$lib/api/realtimeUpdates';
import { createRefreshableResource, type RefreshableLoaderLike, type RefreshableResource } from '$lib/utils/refreshableResource';

export type DomainResource<T> = RefreshableResource<T> & {
  subscribeInvalidations: (
    handler?: Parameters<typeof subscribeDomainInvalidations>[1],
    options?: Parameters<typeof subscribeDomainInvalidations>[2]
  ) => () => void;
};

type InvalidatableResource = Pick<RefreshableResource<unknown>, 'invalidate'>;

export function createDomainResource<T>(options: {
  key: string;
  loader: RefreshableLoaderLike<T>;
  staleMs?: number;
  maxAgeMs?: number;
  kinds: DomainUpdateKind[];
  matches?: (event: RealtimeUpdateEvent) => boolean;
}): DomainResource<T> {
  const resource = createRefreshableResource({
    key: options.key,
    loader: options.loader,
    staleMs: options.staleMs,
    maxAgeMs: options.maxAgeMs
  });

  return {
    ...resource,
    subscribeInvalidations: (handler, subscriptionOptions) =>
      subscribeDomainResourceInvalidations(options.kinds, resource, handler, {
        ...subscriptionOptions,
        matchEvent: options.matches
      })
  };
}

export function subscribeDomainResourceInvalidations(
  kinds: DomainUpdateKind[],
  resources: InvalidatableResource | InvalidatableResource[],
  handler?: Parameters<typeof subscribeDomainInvalidations>[1],
  options?: Parameters<typeof subscribeDomainInvalidations>[2] & {
    matchEvent?: (event: RealtimeUpdateEvent) => boolean;
  }
): () => void {
  const resourceList = Array.isArray(resources) ? resources : [resources];
  return subscribeDomainInvalidations(
    kinds,
    (event) => {
      if (options?.matchEvent && !options.matchEvent(event)) {
        return;
      }
      resourceList.forEach((resource) => resource.invalidate());
      handler?.(event);
    },
    options
  );
}
