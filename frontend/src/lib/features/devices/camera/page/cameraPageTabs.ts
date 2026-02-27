import type { CameraPageTabId } from './cameraPageStateTypes';

type Tab = { id: CameraPageTabId; label: string; ready: boolean };

export const buildCameraTabs = (mediaTabReady: boolean): Tab[] => [
  { id: 'stream', label: 'Stream', ready: true },
  { id: 'controls', label: 'Controls', ready: true },
  { id: 'pipelines', label: 'Pipelines', ready: true },
  { id: 'pose', label: 'Pose', ready: true },
  { id: 'calibration', label: 'Calibration', ready: true },
  { id: 'media', label: 'Media', ready: mediaTabReady }
];
