import type { IconDefinition } from '@fortawesome/free-solid-svg-icons';
import type { PeerIntegrationKind } from '$lib/types/peer';

export type PeerFilterOption = 'all' | PeerIntegrationKind;

export type PeerFilterDefinition = {
  id: PeerFilterOption;
  label: string;
  description: string;
  logo?: string | null;
  icon?: IconDefinition | null;
};

export type CustomMappingForm = {
  poseTranslationX: string;
  poseTranslationY: string;
  poseTranslationZ: string;
  poseRotationRoll: string;
  poseRotationPitch: string;
  poseRotationYaw: string;
  poseTimestamp: string;
  poseLatencyMs: string;
  arucoListPath: string;
  arucoId: string;
  arucoFamily: string;
  arucoCenterX: string;
  arucoCenterY: string;
  arucoTranslationX: string;
  arucoTranslationY: string;
  arucoTranslationZ: string;
  arucoRotationRoll: string;
  arucoRotationPitch: string;
  arucoRotationYaw: string;
};

export type CustomMappingPreview = {
  pose?: {
    translation?: { x?: unknown; y?: unknown; z?: unknown } | null;
    rotation?: { roll?: unknown; pitch?: unknown; yaw?: unknown } | null;
    timestamp?: unknown;
    latencyMs?: unknown;
  } | null;
  arucoTags?: Array<{
    id?: unknown;
    family?: unknown;
    center?: { x?: unknown; y?: unknown } | null;
    translation?: { x?: unknown; y?: unknown; z?: unknown } | null;
    rotation?: { roll?: unknown; pitch?: unknown; yaw?: unknown } | null;
  }>;
};

export type IntegrationPreset = {
  apiBaseUrl: string;
  managementUrl: string;
  streamUrl: string;
  endpointHost: string;
  endpointPort: string;
  kind?: PeerIntegrationKind;
};
