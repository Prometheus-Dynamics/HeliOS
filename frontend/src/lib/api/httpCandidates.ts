import { browser } from '$app/environment';

function uniqueUrls(urls: string[]): string[] {
  const out: string[] = [];
  const seen = new Set<string>();
  for (const value of urls) {
    const trimmed = value.trim();
    if (!trimmed || seen.has(trimmed)) continue;
    seen.add(trimmed);
    out.push(trimmed);
  }
  return out;
}

function parseUrl(url: string): URL | null {
  try {
    if (browser) {
      return new URL(url, globalThis.location?.origin ?? undefined);
    }
    return new URL(url);
  } catch {
    return null;
  }
}

/**
 * Build resilient HTTP candidates for a request URL.
 *
 * Order matters:
 * 1) configured URL (user-selected base must be authoritative)
 * 2) direct API port fallback (:5800 -> :5801)
 */
export function buildHttpCandidateUrls(url: string): string[] {
  const candidates: string[] = [];
  const parsed = parseUrl(url);
  if (!parsed || !parsed.pathname.startsWith('/v1')) {
    return uniqueUrls([url]);
  }

  candidates.push(url);

  if (parsed.port === '5800') {
    const directApi = new URL(parsed.toString());
    directApi.port = '5801';
    candidates.push(directApi.toString());
  }

  return uniqueUrls(candidates);
}
