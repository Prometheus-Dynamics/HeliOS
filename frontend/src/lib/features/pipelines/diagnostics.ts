import type { PipelineDiagnostics, PipelineDiagnosticWarning } from '$lib/types/pipeline';

const trimmedString = (value: unknown): string | null => {
  if (typeof value !== 'string') return null;
  const trimmed = value.trim();
  return trimmed.length > 0 ? trimmed : null;
};

const normalizeWarning = (value: unknown): PipelineDiagnosticWarning | null => {
  if (!value || typeof value !== 'object') return null;
  const record = value as Record<string, unknown>;
  const message = trimmedString(record.message);
  if (!message) return null;
  const nodeId = trimmedString(record.nodeId);
  const port = trimmedString(record.port);
  return {
    message,
    ...(nodeId ? { nodeId } : {}),
    ...(port ? { port } : {})
  };
};

export function normalizeDiagnostics(input: unknown): PipelineDiagnostics | null {
  if (!input || typeof input !== 'object') return null;
  const record = input as Record<string, unknown>;
  const warnings = Array.isArray(record.warnings)
    ? record.warnings
        .map((warning) => normalizeWarning(warning))
        .filter((warning): warning is PipelineDiagnosticWarning => Boolean(warning))
    : [];
  const error = trimmedString(record.error);
  if (warnings.length === 0 && !error) return null;
  return {
    warnings,
    ...(error ? { error } : {})
  };
}

export function cloneDiagnostics(diagnostics: PipelineDiagnostics | null | undefined): PipelineDiagnostics | null {
  if (!diagnostics) return null;
  return {
    warnings: diagnostics.warnings.map((warning) => ({
      message: warning.message,
      ...(warning.nodeId ? { nodeId: warning.nodeId } : {}),
      ...(warning.port ? { port: warning.port } : {})
    })),
    ...(diagnostics.error ? { error: diagnostics.error } : {})
  };
}
