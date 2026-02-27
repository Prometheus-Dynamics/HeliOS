type LogEntry = {
  level?: string;
};

type LogFilterPayload = {
  entries: LogEntry[];
  filter: string;
};

self.onmessage = (event: MessageEvent<LogFilterPayload>) => {
  const { entries, filter } = event.data;
  const normalized = Array.isArray(entries) ? entries : [];
  const filtered = filter === 'all' ? normalized : normalized.filter((entry) => entry.level === filter);
  self.postMessage(filtered);
};

export {};
