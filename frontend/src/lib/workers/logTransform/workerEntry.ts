import type { LogEntry, LogFramePayload, LogTransformRequest, LogTransformResponse } from './types';
import { decorateLogMessage } from './logDecorators';
import { findLevelColor, resolveLogLevel } from './levelDetection';

function normalizeLogFrame(frame: LogFramePayload, fallbackStreamId: string): LogEntry | null {
  const streamId = frame.stream_id ?? fallbackStreamId;
  const identifier =
    frame.sequence != null ? String(frame.sequence) : frame.timestamp ?? Math.random().toString(36).slice(2);
  const fieldsRaw = frame.fields;
  const fields =
    fieldsRaw && typeof fieldsRaw === 'object' && !Array.isArray(fieldsRaw)
      ? (fieldsRaw as Record<string, unknown>)
      : {};
  const rawMessage = frame.message ?? '(no message)';
  const decorated = decorateLogMessage(rawMessage);
  const resolvedLevel = resolveLogLevel(frame.level, decorated.plainText, decorated.levelMarker);
  const levelColor = resolvedLevel.color ?? findLevelColor(decorated.segments, resolvedLevel.level);
  return {
    id: `${streamId}-${identifier}`,
    level: resolvedLevel.level,
    levelColor,
    threadId: decorated.threadId,
    message: rawMessage,
    timestamp: frame.timestamp ?? '',
    fileId: frame.file_id ?? undefined,
    fields,
    inlineTimestamp: decorated.inlineTimestamp,
    renderedMessage: decorated.rendered
  };
}

function parseNdjson(payload: string, fallbackStreamId: string): LogEntry[] {
  const entries: LogEntry[] = [];
  const lines = payload.split('\n');
  for (const line of lines) {
    const trimmed = line.trim();
    if (!trimmed) continue;
    try {
      const frame = JSON.parse(trimmed) as LogFramePayload & { heartbeat?: boolean };
      if (frame.heartbeat) continue;
      const entry = normalizeLogFrame(frame, fallbackStreamId);
      if (entry) {
        entries.push(entry);
      }
    } catch {
      // ignore malformed frames
    }
  }
  return entries;
}

self.onmessage = (event: MessageEvent<LogTransformRequest>) => {
  const { type, requestId, payload, streamId } = event.data;
  if (type === 'snapshot') {
    const entries = parseNdjson(payload, streamId);
    const response: LogTransformResponse = { type, requestId, entries };
    self.postMessage(response);
    return;
  }
  if (type === 'frame') {
    let entries: LogEntry[] = [];
    try {
      const frame = JSON.parse(payload) as LogFramePayload & { heartbeat?: boolean };
      if (!frame.heartbeat) {
        const entry = normalizeLogFrame(frame, streamId);
        if (entry) entries = [entry];
      }
    } catch {
      // ignore malformed frames
    }
    const response: LogTransformResponse = { type, requestId, entries };
    self.postMessage(response);
  }
};
