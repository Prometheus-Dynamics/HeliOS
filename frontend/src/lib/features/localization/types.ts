export type CameraExtrinsics = {
  position: { x: number; y: number; z: number };
  rotation: { roll: number; pitch: number; yaw: number };
};

export type CustomFieldOrigin = {
  id: string;
  name: string;
  x: number;
  z: number;
  yawDeg: number;
};

export type CustomField = {
  id: string;
  name: string;
  width: number;
  depth: number;
  origins: CustomFieldOrigin[];
  mapId?: string | null;
};
