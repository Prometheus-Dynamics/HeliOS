// `peripheral` events (helios-api pushes the Lemnos bridge's device readings as lemnosd publishes
// them): `{id, values: {key: value}, expired: [key]}`, one per device. The live store hands them
// here; whatever shows peripherals listens, without the stores importing each other.

export interface PeripheralEvent {
  id: string;
  values: Record<string, unknown>;
  expired: string[];
}

type Listener = (event: PeripheralEvent) => void;

const listeners = new Set<Listener>();

export function onPeripheral(listener: Listener): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

export function emitPeripheral(event: PeripheralEvent): void {
  for (const listener of listeners) listener(event);
}
