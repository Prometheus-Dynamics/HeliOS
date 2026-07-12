export const PROFILE_COLORS = [
  '#7c3aed',
  '#2563eb',
  '#0f766e',
  '#ca8a04',
  '#dc2626',
  '#9333ea',
  '#0891b2',
  '#65a30d'
] as const;

export function profileColorForId(
  profileId: string,
  profiles: Array<{ id: string }>,
  profileIndexById:
    | Map<string, number>
    | { get: (key: string) => number | undefined }
    | null
    | undefined,
  palette: readonly string[] = PROFILE_COLORS
): string {
  const paletteSafe = palette.length ? palette : PROFILE_COLORS;
  const indexed = profileIndexById?.get(profileId);
  if (indexed != null && Number.isFinite(indexed)) {
    return paletteSafe[Math.abs(indexed) % paletteSafe.length];
  }

  const fallbackIndex = profiles.findIndex((profile) => profile.id === profileId);
  if (fallbackIndex >= 0) {
    return paletteSafe[fallbackIndex % paletteSafe.length];
  }

  let hash = 0;
  for (const ch of profileId) {
    hash = (hash * 31 + ch.charCodeAt(0)) | 0;
  }
  return paletteSafe[Math.abs(hash) % paletteSafe.length];
}
