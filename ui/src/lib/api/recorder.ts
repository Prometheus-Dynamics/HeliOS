// The recordings' backend. The device's helios-api in live mode; with `?mock=1`
// the simulated recorder in mock-recorder.ts. The recordings store talks only to
// this interface, so the screens run the same in both.

import { api, recordingDownloadUrl } from "./client";
import type { EidosExport, PartSummary, RecorderSettings, RecordingListing, RecordingStatus } from "./recording-types";

export interface RecorderBackend {
  listing(): Promise<RecordingListing>;
  start(camera: string): Promise<RecordingStatus>;
  stop(id: string): Promise<RecordingStatus>;
  remove(id: string): Promise<void>;
  captureLast(body: { seconds?: number; camera?: string }): Promise<PartSummary>;
  exportEidos(id: string): Promise<EidosExport>;
  settings(): Promise<RecorderSettings>;
  saveSettings(settings: RecorderSettings): Promise<RecorderSettings>;
  /** A link that downloads a stored recording; null when there is nothing to download (mock). */
  downloadUrl(id: string): string | null;
}

export const liveRecorder: RecorderBackend = {
  listing: () => api.recordings(),
  start: (camera) => api.startRecording(camera),
  stop: (id) => api.stopRecording(id),
  remove: (id) => api.deleteRecording(id),
  captureLast: (body) => api.captureLast(body),
  exportEidos: (id) => api.exportEidos(id),
  settings: () => api.recordingSettings(),
  saveSettings: (settings) => api.setRecordingSettings(settings),
  downloadUrl: (id) => recordingDownloadUrl(id),
};
