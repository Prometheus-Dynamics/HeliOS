import { DEFAULT_ROBOT_DIMENSIONS } from '$lib/3d/rig';
import type { FieldMapOverlay } from '$lib/features/localization/fieldMaps';
import type { RigCameraInfo, RobotDimensions } from '$lib/types/rig';

export type ArucoBitGrid = {
  width: number;
  border: number;
  rows: string[];
};

export type PoseQuaternion = {
  x: number;
  y: number;
  z: number;
  w: number;
};

type LocalizationMarkerBase = {
  id: string;
  label: string;
  position: [number, number, number];
  quaternion?: PoseQuaternion;
  heading?: number;
  color?: string;
  status?: string;
  source?: {
    id: string;
    streamId?: string;
    outputKey?: string;
    streamLabel?: string;
    cameraUid?: string;
    cameraPath?: string;
    pipelineId?: string;
    pipelineLabel?: string;
    sourceIndex?: number;
    profileId?: string;
  };
};

export type ArucoMarker = LocalizationMarkerBase & {
  targetType?: 'aruco' | 'aruco-plane';
  tagId?: number | string;
  tagSize?: number;
  tagHeight?: number;
  tagBorderRatio?: number;
  tagBits?: ArucoBitGrid;
  codeRotation?: number;
};

export type PolygonMarker = LocalizationMarkerBase & {
  targetType: 'polygon';
  outline: Array<[number, number]>;
  thickness?: number;
  outlineColor?: string;
  fillOpacity?: number;
};

export type LocalizationMarker = ArucoMarker | PolygonMarker;

export type LocalizationViewMode = 'isolated' | 'frc-field' | 'custom-field';

export type LocalizationFieldDefinition = {
  name: string;
  width: number;
  depth: number;
  overlay?: FieldMapOverlay | null;
};

export type RobotOverlay = {
  id: string;
  label?: string;
  color?: string;
  transform: { position: [number, number, number]; quaternion?: PoseQuaternion } | null;
};

// Viewer field plane uses X (left/right) as "width" and Z (forward/back) as "depth".
// WPILib field coords are X forward (length) and Y left (width), so depth=length and width=width.
export const FRC_FIELD_DIMENSIONS = { width: 8.2296, depth: 16.4592 };
export const DEFAULT_CUSTOM_FIELD: LocalizationFieldDefinition = { name: 'Custom field', width: 10, depth: 6 };
export const DEFAULT_ROBOT_WIDTH_M = DEFAULT_ROBOT_DIMENSIONS.width;
export const DEFAULT_ROBOT_LENGTH_M = DEFAULT_ROBOT_DIMENSIONS.length;
export const BUMPER_THICKNESS_M = DEFAULT_ROBOT_DIMENSIONS.bumperThickness;
export const ROBOT_HEIGHT_M = DEFAULT_ROBOT_DIMENSIONS.bumperHeight;
export const GROUND_CLEARANCE_M = DEFAULT_ROBOT_DIMENSIONS.groundClearance;
export const DEFAULT_BUMPER_COLOR = 0x991b1b;

export type LocalizationViewerProps = {
  markers?: LocalizationMarker[];
  tagLineMarkers?: LocalizationMarker[];
  referenceMarkers?: LocalizationMarker[];
  mode?: LocalizationViewMode;
  showOriginAxes?: boolean;
  showTagLines?: boolean;
  showFieldImage?: boolean;
  showMinimapTrail?: boolean;
  bumperNumber?: string;
  bumperColor?: string | null;
  robotOverlays?: RobotOverlay[];
  robot?: RobotDimensions;
  cameras?: RigCameraInfo[];
  customField?: LocalizationFieldDefinition | null;
  showRobot?: boolean;
  showCameras?: boolean;
  cameraGhostActive?: boolean;
  sceneTransform?: { position: [number, number, number]; quaternion?: PoseQuaternion } | null;
  robotTransform?: { position: [number, number, number]; quaternion?: PoseQuaternion } | null;
  cameraTransforms?: Record<string, { position: [number, number, number]; quaternion?: PoseQuaternion }> | null;
  cameraHighlightColor?: string | null;
  minimapPoseDot?: { position: [number, number, number]; color?: string | null } | null;
  cameraPovEnabled?: boolean;
  cameraPovTransform?: { position: [number, number, number]; quaternion?: PoseQuaternion } | null;
  cameraPovIntrinsics?: { fx: number; fy: number; cx: number; cy: number; width: number; height: number } | null;
  cameraPovApplyFov?: boolean;
  cameraPovForwardSign?: 1 | -1;
  robotFollowPovEnabled?: boolean;
};
