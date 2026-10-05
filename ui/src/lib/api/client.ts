// The HeliOS client: everything the UI reads and does goes through one
// `ClusterClient`. Today it is the mock in `mock.svelte.ts`; the real one talks
// to the device over Orion's HTTP/WebSocket adapter with the same shapes.

export function errorText(error: unknown): string {
  if (error instanceof Error) return error.message;
  if (typeof error === "string") return error;
  return "Something went wrong";
}
