declare module '@asyncapi/react-component/browser/standalone' {
  const AsyncApiStandalone: {
    render: (options: unknown, element?: HTMLElement) => void;
    hydrate?: (options: unknown, element?: HTMLElement) => void;
    hljs?: unknown;
  };
  export = AsyncApiStandalone;
}
