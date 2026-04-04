export function createMergedView<TView extends object>(...sources: object[]): TView {
  const ownKeys = (): Array<string | symbol> => {
    const keys = new Set<string | symbol>();
    for (const source of sources) {
      for (const key of Reflect.ownKeys(source)) {
        if (typeof key === 'string' || typeof key === 'symbol') {
          keys.add(key);
        }
      }
    }
    return Array.from(keys);
  };

  return new Proxy({} as TView, {
    get(_target, property, receiver) {
      for (let index = sources.length - 1; index >= 0; index -= 1) {
        const source = sources[index];
        if (property in source) {
          return Reflect.get(source, property, receiver);
        }
      }
      return undefined;
    },
    set(_target, property, value, receiver) {
      for (let index = sources.length - 1; index >= 0; index -= 1) {
        const source = sources[index];
        if (property in source) {
          return Reflect.set(source, property, value, receiver);
        }
      }
      return false;
    },
    has(_target, property) {
      return sources.some((source) => property in source);
    },
    ownKeys() {
      return ownKeys();
    },
    getOwnPropertyDescriptor(_target, property) {
      for (let index = sources.length - 1; index >= 0; index -= 1) {
        const source = sources[index];
        const descriptor = Reflect.getOwnPropertyDescriptor(source, property);
        if (descriptor) {
          return { ...descriptor, configurable: true };
        }
      }
      return undefined;
    }
  });
}
