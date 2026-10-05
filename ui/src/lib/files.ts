// Download and pick files without a server round trip.

export function downloadBlob(name: string, blob: Blob) {
  const url = URL.createObjectURL(blob);
  const a = document.createElement("a");
  a.href = url;
  a.download = name;
  a.click();
  setTimeout(() => URL.revokeObjectURL(url), 1000);
}

export function downloadJson(name: string, value: unknown) {
  downloadBlob(name, new Blob([JSON.stringify(value, null, 2)], { type: "application/json" }));
}

export function downloadText(name: string, text: string, type = "text/plain") {
  downloadBlob(name, new Blob([text], { type }));
}

export function pickFile(accept: string): Promise<File | null> {
  return new Promise((resolve) => {
    const input = document.createElement("input");
    input.type = "file";
    input.accept = accept;
    input.onchange = () => resolve(input.files?.[0] ?? null);
    input.oncancel = () => resolve(null);
    input.click();
  });
}

export async function pickJson<T>(): Promise<T | null> {
  const file = await pickFile(".json,application/json");
  if (!file) return null;
  try {
    return JSON.parse(await file.text()) as T;
  } catch {
    return null;
  }
}
