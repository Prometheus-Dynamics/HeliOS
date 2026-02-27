import { classifyMediaKind, type MediaAssetType } from '$lib/features/media/mediaKind';

export type DataTransferItemWithEntry = DataTransferItem & {
  webkitGetAsEntry?: () => FileSystemEntry | null;
};

function isFileEntry(entry: FileSystemEntry): entry is FileSystemFileEntry {
  return entry.isFile;
}

function isDirectoryEntry(entry: FileSystemEntry): entry is FileSystemDirectoryEntry {
  return entry.isDirectory;
}

export function getFileExtension(name: string): string {
  return name.split('.').pop()?.toLowerCase() ?? '';
}

export function fileDisplayName(file: File): string {
  const withPath = file as File & { webkitRelativePath?: string };
  return withPath.webkitRelativePath && withPath.webkitRelativePath.length ? withPath.webkitRelativePath : file.name;
}

export function createFileListFromArray(files: File[]): FileList {
  const dataTransfer = new DataTransfer();
  files.forEach((file) => dataTransfer.items.add(file));
  return dataTransfer.files;
}

export function detectMediaKind(file: File): MediaAssetType {
  return classifyMediaKind({ name: file.name, contentType: file.type });
}

export function inferKindFromFile(file: File): MediaAssetType {
  return detectMediaKind(file);
}

export function sumFileSizes(files: File[]): number {
  return files.reduce((sum, file) => sum + (Number.isFinite(file.size) ? file.size : 0), 0);
}

export async function extractFilesFromDataTransfer(dataTransfer: DataTransfer | null): Promise<File[]> {
  if (!dataTransfer) return [];
  const items = Array.from(dataTransfer.items ?? []);
  const entries = items
    .filter((item) => item.kind === 'file')
    .map((item) => (item as DataTransferItemWithEntry).webkitGetAsEntry?.())
    .filter((entry): entry is FileSystemEntry => entry != null);
  if (!entries.length) {
    return Array.from(dataTransfer.files ?? []);
  }
  const files = await Promise.all(entries.map((entry) => collectFilesFromEntry(entry)));
  return files.flat();
}

export async function collectFilesFromEntry(entry: FileSystemEntry): Promise<File[]> {
  if (isFileEntry(entry)) {
    return new Promise<File[]>((resolve, reject) => {
      entry.file((file) => resolve([file]), reject);
    });
  }
  if (!isDirectoryEntry(entry)) return [];
  const reader = entry.createReader();
  const entries = await readAllDirectoryEntries(reader);
  const files = await Promise.all(entries.map((child) => collectFilesFromEntry(child)));
  return files.flat();
}

export async function readAllDirectoryEntries(reader: FileSystemDirectoryReader): Promise<FileSystemEntry[]> {
  const entries: FileSystemEntry[] = [];
  while (true) {
    const batch = await new Promise<FileSystemEntry[]>((resolve, reject) => {
      reader.readEntries(resolve, reject);
    });
    if (!batch.length) break;
    entries.push(...batch);
  }
  return entries;
}
