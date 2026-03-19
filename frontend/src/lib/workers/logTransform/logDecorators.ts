import type { DecoratedLogMessage, InlineTimestampDetails, LogSegment } from './types';
import { parseAnsiSegments, removeSegmentRange, segmentsToHtml, trimLeadingSegments } from './ansiParser';
import { detectLeadingLevelPrefix, extractLevelMarker, levelLineRemovalLength } from './levelDetection';

export function decorateLogMessage(raw: string): DecoratedLogMessage {
  const segments = parseAnsiSegments(raw);
  const sourceSegments = segments;
  const plainWithLevel = segments.map((segment) => segment.text).join('');
  const levelMarker = extractLevelMarker(segments);

  let workingSegments = segments;
  let workingPlain = plainWithLevel;

  const inline = extractInlineTimestamp(workingPlain);
  if (inline) {
    workingSegments = trimLeadingSegments(workingSegments, inline.removeUntil);
    workingPlain = workingPlain.slice(inline.removeUntil);
  }

  const stripped = stripLeadingLevelLines(workingSegments, workingPlain);
  workingSegments = stripped.segments;
  workingPlain = stripped.remainingPlain;

  const threadDetails = stripThreadIdMarker(workingSegments, workingPlain);
  workingSegments = threadDetails.segments;
  workingPlain = threadDetails.remainingPlain;

  return {
    inlineTimestamp: inline?.value ?? null,
    rendered: segmentsToHtml(workingSegments),
    plainText: plainWithLevel,
    levelMarker: levelMarker ?? undefined,
    segments: sourceSegments,
    threadId: threadDetails.threadId
  };
}

export function extractInlineTimestamp(message: string): InlineTimestampDetails | null {
  if (!message) return null;
  const leadingWhitespaceMatch = message.match(/^\s*/);
  const leadingWhitespace = leadingWhitespaceMatch ? leadingWhitespaceMatch[0].length : 0;
  const trimmed = message.slice(leadingWhitespace);
  const isoMatch = trimmed.match(/^(\d{4}-\d{2}-\d{2}T[0-9:.+-]+Z?)/);
  if (!isoMatch) return null;
  let removeUntil = leadingWhitespace + isoMatch[1].length;
  while (removeUntil < message.length && /\s/.test(message[removeUntil])) {
    removeUntil += 1;
  }
  return { value: isoMatch[1], removeUntil };
}

export function stripLeadingLevelLines(
  initialSegments: LogSegment[],
  initialPlain: string
): { segments: LogSegment[]; remainingPlain: string } {
  let segments = initialSegments;
  let plain = initialPlain;

  while (true) {
    const prefix = detectLeadingLevelPrefix(plain);
    if (!prefix) {
      break;
    }
    const removal = levelLineRemovalLength(plain, prefix.removeUntil);
    segments = trimLeadingSegments(segments, removal);
    plain = plain.slice(removal);
  }

  return { segments, remainingPlain: plain };
}

export function stripThreadIdMarker(
  segments: LogSegment[],
  plain: string
): { segments: LogSegment[]; remainingPlain: string; threadId?: string } {
  const match = plain.match(/ThreadId\((\d+)\)/);
  if (!match || match.index == null) {
    return { segments, remainingPlain: plain };
  }
  const start = match.index;
  let end = start + match[0].length;
  while (end < plain.length && /[\s:]/.test(plain[end] ?? '')) {
    end += 1;
  }
  const length = end - start;
  const trimmedSegments = removeSegmentRange(segments, start, length);
  const updatedPlain = plain.slice(0, start) + plain.slice(end);
  return { segments: trimmedSegments, remainingPlain: updatedPlain, threadId: match[1] };
}
