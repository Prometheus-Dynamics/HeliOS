import type { RealtimeUpdateDomain, RealtimeUpdateEvent, RealtimeUpdateKind } from '$lib/api/realtimeUpdates';
import { createRefreshableResource, type RefreshableLoaderLike, type RefreshableResource } from '$lib/utils/refreshableResource';

type DomainUpdateKind = RealtimeUpdateKind | RealtimeUpdateDomain;
type DomainInvalidationHandler = (event: RealtimeUpdateEvent) => void;
type DomainInvalidationOptions = {
  debounceMs?: number;
};

export type DomainResource<T> = RefreshableResource<T> & {
  subscribeInvalidations: (
    handler?: DomainInvalidationHandler,
    options?: DomainInvalidationOptions
  ) => () => void;
};

type InvalidatableResource = Pick<RefreshableResource<unknown>, 'invalidate'>;

function subscribeDomainInvalidationsLazy(
  kinds: DomainUpdateKind[],
  handler: DomainInvalidationHandler,
  options?: DomainInvalidationOptions
): () => void {
  let closed = false;
  let stop: (() => void) | null = null;

  void import('$lib/api/invalidation')
    .then(({ subscribeDomainInvalidations }) => {
      if (closed) return;
      stop = subscribeDomainInvalidations(kinds, handler, options);
    })
    .catch(() => {
      stop = null;
    });

  return () => {
    closed = true;
    stop?.();
    stop = null;
  };
}

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
  handler?: DomainInvalidationHandler,
  options?: DomainInvalidationOptions & {
    matchEvent?: (event: RealtimeUpdateEvent) => boolean;
  }
): () => void {
  const resourceList = Array.isArray(resources) ? resources : [resources];
  return subscribeDomainInvalidationsLazy(
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
