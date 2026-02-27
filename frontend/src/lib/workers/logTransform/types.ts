export type LogFilter = 'all' | 'info' | 'warn' | 'error' | 'debug';

export type LogSegment = {
  text: string;
  color?: string;
  background?: string;
  bold?: boolean;
  dim?: boolean;
  italic?: boolean;
  underline?: boolean;
};

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

export type LogFramePayload = {
  stream_id?: string;
  sequence?: number;
  timestamp?: string;
  level?: string;
  message?: string;
  file_id?: string;
  fields?: unknown;
};

export type InlineTimestampDetails = {
  value: string;
  removeUntil: number;
};

export type LevelMarker = {
  level: Exclude<LogFilter, 'all'>;
  color?: string;
};

export type DecoratedLogMessage = {
  inlineTimestamp: string | null;
  rendered: string;
  plainText: string;
  levelMarker?: LevelMarker;
  segments: LogSegment[];
  threadId?: string;
};

export type LogTransformRequest =
  | { type: 'snapshot'; requestId: number; payload: string; streamId: string }
  | { type: 'frame'; requestId: number; payload: string; streamId: string };

export type LogTransformResponse = {
  type: 'snapshot' | 'frame';
  requestId: number;
  entries: LogEntry[];
};

export type SegmentState = {
  color?: string;
  background?: string;
  bold?: boolean;
  dim?: boolean;
  italic?: boolean;
  underline?: boolean;
};
