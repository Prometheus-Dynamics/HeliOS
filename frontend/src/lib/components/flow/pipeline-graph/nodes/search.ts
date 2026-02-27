import type { PipelineDataType } from '$lib/types/pipeline';

const normalizeSearchValue = (value: string | null | undefined): string | null => {
  if (!value) return null;
  const trimmed = value.trim().toLowerCase();
  return trimmed.length > 0 ? trimmed : null;
};

export const matchesSearchTokens = (tokens: string[], values: Array<string | null | undefined>): boolean => {
  if (tokens.length === 0) return false;
  const haystack = values
    .map((value) => normalizeSearchValue(value))
    .filter(Boolean)
    .join(' ');
  if (!haystack) return false;
  return tokens.every((token) => haystack.includes(token));
};

export const appendDataTypeTokens = (target: string[], dataType: PipelineDataType | undefined) => {
  if (!dataType) return;
  if (typeof dataType === 'string') {
    target.push(dataType);
    return;
  }
  const typed = dataType as {
    kind?: string;
    label?: string;
    summary?: string;
    format?: string;
    descriptor?: { label?: string; summary?: string };
  };
  if (typed.kind) target.push(typed.kind);
  if (typed.label) target.push(typed.label);
  if (typed.summary) target.push(typed.summary);
  if (typed.format) target.push(typed.format);
  if (typed.descriptor?.label) target.push(typed.descriptor.label);
  if (typed.descriptor?.summary) target.push(typed.descriptor.summary);
};
