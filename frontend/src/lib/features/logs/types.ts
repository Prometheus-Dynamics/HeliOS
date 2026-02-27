export type LogFilter = 'all' | 'info' | 'warn' | 'error' | 'debug';

export type LogEntry = {
  id: string;
  level: Exclude<LogFilter, 'all'>;
  levelColor?: string;
  threadId?: string;
  message: string;
  timestamp: string;
  fileId?: string;
  fields?: Record<string, unknown>;
  inlineTimestamp: string | null;
  renderedMessage: string;
};

export type LogStreamSummary = {
  id: string;
  label: string;
  rotation_max_bytes?: number | null;
  retention_count?: number | null;
};
