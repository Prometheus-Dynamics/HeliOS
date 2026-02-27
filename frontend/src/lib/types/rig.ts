export type PoseVector = {
  x: number;
  y: number;
  z: number;
};

export type PoseRotation = {
  roll: number;
  pitch: number;
  yaw: number;
};

export type RigPose = {
  translation: PoseVector;
  rotation: PoseRotation;
  updatedAt?: string | null;
};

export type RobotDimensions = {
  width: number;
  length: number;
  bumperHeight: number;
  bumperThickness: number;
  groundClearance: number;
};

export type RigCameraInfo = {
  uid: string;
  streamId: string | null;
  cameraUid: string | null;
  streamAlias?: string | null;
  driverCameraId: string;
  displayName: string;
  backend: string;
  hardwareId?: string | null;
  pose?: RigPose;
};

export type RigLayout = {
  robot: RobotDimensions;
  cameras: RigCameraInfo[];
};
