export const EXPECTED_UPLOAD_BYTES_HEADER = 'x-helios-upload-bytes';

export function uploadSizeHeaders(file: Blob): Record<string, string> {
  return { [EXPECTED_UPLOAD_BYTES_HEADER]: String(file.size) };
}

export function verifyUploadedBytes(expectedBytes: number, storedBytes: unknown, action: string): void {
  if (typeof storedBytes !== 'number' || !Number.isFinite(storedBytes)) {
    throw new Error(`${action} completed but the server did not report a stored size.`);
  }
  if (storedBytes !== expectedBytes) {
    throw new Error(`${action} verification failed: expected ${expectedBytes} bytes but server stored ${storedBytes}.`);
  }
}

export function normalizeUploadError(error: unknown, action: string): Error {
  if (isTransportUploadError(error)) {
    return new Error(`${action} did not finish cleanly. Upload status is unknown; reconnect and verify before retrying.`);
  }
  if (error instanceof Error) {
    return error;
  }
  return new Error(String(error));
}

function isTransportUploadError(error: unknown): boolean {
  if (error instanceof DOMException && error.name === 'AbortError') {
    return true;
  }
  if (!(error instanceof Error)) {
    return false;
  }
  const message = error.message.trim().toLowerCase();
  return (
    message === 'request timed out' ||
    message.includes('network error during upload') ||
    message.includes('failed to fetch') ||
    message.includes('networkerror') ||
    message.includes('load failed')
  );
}
