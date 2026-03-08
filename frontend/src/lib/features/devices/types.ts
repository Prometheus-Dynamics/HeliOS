import type { CameraCard, CameraStatus, PeripheralEntry } from '$lib/types/devices';

export type ThrottleBanner = { label: string; detail: string; tone: 'warning' | 'error' };

export type CameraCardItem = {
  id: string;
  name: string;
  status: CameraCard['status'];
  recordingActive?: boolean;
  recordingSinceMs?: number | null;
  pipeline: string | null;
  resolution: string;
  bandwidth: string;
  lastSeen: string;
  driverNamespace: string;
  captureSessionId: string | null;
  captureSessionAlias: string | null;
  cameraUid: string;
  href: '/devices' | '/peers' | `/devices/${string}`;
  statusClass: string;
};

export type PeripheralBadgeTone = 'neutral' | 'success' | 'warning' | 'error';

export type PeripheralBadge = {
  label: string;
  tone?: PeripheralBadgeTone;
  description?: string | null;
};

export type PeripheralItem = {
  id: string;
  name: string;
  driverNamespace?: string | null;
  driverCameraId?: string | null;
  status?: string | null;
  interval?: string | null;
  type?: string | null;
  icon?: PeripheralEntry['icon'] | null;
  badges?: PeripheralBadge[] | null;
  payload?: PeripheralEntry;
};

export type CameraStatusCounts = Record<CameraStatus, number>;
