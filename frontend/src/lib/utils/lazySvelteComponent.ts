export function createLazySvelteComponentLoader<T>(
  loader: () => Promise<{ default: T }>
): {
  current: () => T | null;
  load: () => Promise<T>;
} {
  let component: T | null = null;
  let inFlight: Promise<T> | null = null;

  return {
    current: () => component,
    load: () => {
      if (component) {
        return Promise.resolve(component);
      }
      if (inFlight) {
        return inFlight;
      }
      inFlight = loader()
        .then((module) => {
          component = module.default;
          return component;
        })
        .finally(() => {
          inFlight = null;
        });
      return inFlight;
    }
  };
}
