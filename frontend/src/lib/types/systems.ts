import type { SummaryTile } from './devices';
import type { RigLayout } from './rig';
import type { SensorOrientation } from './devices';

export type ImuAxes = {
  x: number;
  y: number;
  z: number;
};

export type ImuSources = {
  accelGyro?: string | null;
  magnetometer?: string | null;
};

export type ImuOptions = {
  fusion: string[];
  range: string[];
  intervalsMs: number[];
};

export type ImuStatus = {
  fusion: string;
  range: string;
  updateIntervalMs: number;
  drVelocityDampTauSeconds: number;
  drStillVelocityZeroTauSeconds: number;
  drMaxAccelWorldMps2: number;
  drMaxSpeedMps: number;
  drMaxPositionM: number;
  drLockPosition: boolean;
  updatedAt: string | null;
  dtSeconds: number | null;
  lastError?: string | null;
  hasSample: boolean;
  orientation: SensorOrientation;
  linearAccel: ImuAxes;
  correctedWorldAccelMps2: ImuAxes;
  velocityWorld: ImuAxes;
  velocityDeltaWorld: ImuAxes;
  linearSpeedMps: number;
  linearSpeedNormalized: number;
  positionWorld: ImuAxes;
  angularVelocityDps: ImuAxes;
  angularSpeedDps: number;
  angularSpeedNormalized: number;
  isMoving: boolean;
  isMovingFast: boolean;
  motionG: number;
  motionFastG: number;
  motionFastThresholdG: number;
  motionNoiseFloorG: number;
  drConfidence: number;
  accel: ImuAxes;
  gyro: ImuAxes;
  mag: ImuAxes | null;
  sources?: ImuSources | null;
  options?: ImuOptions | null;
};

export type HealthIssue = {
  code: string;
  description: string;
};

export type HealthSnapshot = {
  status: string;
  checkedAt: string | null;
  issues: HealthIssue[];
};

export type SoftwareVersions = {
  api: string;
  engine: string;
  updater: string;
  buildId: string;
  binaries?: BinaryVersion[];
};

export type BinaryVersion = {
  id: string;
  version: string;
};

export type LogPolicy = {
  rotationMaxBytes: number;
  rotationMaxAgeSecs: number;
  retentionCount: number;
};

export type DeviceOverview = {
  hostname: string;
  teamNumber: number | null;
  deviceId: string;
  serial: string;
  sku: string;
  health: HealthSnapshot;
  software: SoftwareVersions | null;
  logPolicy: LogPolicy | null;
};

export type NetworkInterface = {
  name: string;
  mode: 'dhcp' | 'static';
  mac?: string | null;
  address?: string | null;
  prefix?: number | null;
  gateway?: string | null;
  leaseLabel?: string | null;
};

export type CaptureSessionSummary = {
  sessionId: string;
  alias: string | null;
  manifestPath: string;
  cameraDriverId: string;
  cameraUid: string;
  encodingEnabled: boolean;
  clientCount: number;
  backend: string | null;
  format: string | null;
  fps: number | null;
  latencyMs: number | null;
  pipelines: string[];
};

export type LogStreamEntry = {
  id: string;
  label: string;
  rotationMaxBytes: number;
  retentionCount: number;
};

export type PlatformFamily = 'raspberry_pi' | 'generic_linux' | 'unknown';

export type SystemsPlatformIdentity = {
  family: PlatformFamily;
  model: string | null;
  architecture: string;
};

export type SystemsCapabilitySnapshot = {
  logs: boolean;
  console: boolean;
  processes: boolean;
  sensors: boolean;
  i2c: boolean;
  imu: boolean;
  updater: boolean;
  resourceGuard: boolean;
  activeRoot: boolean;
};

export type SystemsTokioRuntimePolicy = {
  workerThreads: number;
  maxBlockingThreads: number;
  threadStackBytes: number | null;
  blockingKeepAliveMs: number | null;
};

export type SystemsStartupCacheWarmPolicy = {
  initialDelayMs: number;
  retryDelayMs: number;
  attempts: number;
};

export type SystemsLogSourcesPolicy = {
  cacheMs: number;
  refreshTimeoutMs: number;
};

export type SystemsI2cInventoryPolicy = {
  timeoutMs: number;
  cacheTtlMs: number;
};

export type SystemsImuRuntimePolicy = {
  idleIntervalMs: number;
};

export type SystemsResourceGuardPolicy = {
  enabled: boolean;
  pollMs: number;
  memLowKb: number;
  memRecoverKb: number;
  cooldownMs: number;
  metricsTopN: number;
  metricsTimeoutMs: number;
  allowStopFallback: boolean;
  stopTimeoutMs: number;
};

export type SystemsStyxCaptureTunables = {
  queueDepth: number | null;
  poolMin: number | null;
  poolBytes: number | null;
  poolSpare: number | null;
  anyOverridden: boolean;
};

export type SystemsRuntimePolicies = {
  logFilter: string;
  apiTokio: SystemsTokioRuntimePolicy;
  engineTokio: SystemsTokioRuntimePolicy;
  peripheralsTokio: SystemsTokioRuntimePolicy;
  startupCacheWarm: SystemsStartupCacheWarmPolicy;
  logSources: SystemsLogSourcesPolicy;
  i2cInventory: SystemsI2cInventoryPolicy;
  imu: SystemsImuRuntimePolicy;
  resourceGuard: SystemsResourceGuardPolicy;
  styxCapture: SystemsStyxCaptureTunables;
};

export type SystemsLogSourcesFreshness = {
  state: string;
  reason: string;
  observedAtMs: number;
  lastSuccessAtMs: number | null;
};

export type SystemsObservabilityHealth = {
  ok: boolean;
  serverTimeMs: number;
  uptimeMs: number;
  version: string;
  shadowRecorder: boolean;
  pipelineRegistryStartupWarm: boolean;
  pipelineRegistryPrefetch: boolean;
  apiToolsHelperOk: boolean;
  apiToolsHelperPath: string;
};

export type SystemsObservabilityStreams = {
  streamCount: number;
  codecCount: number;
  stale: boolean;
  revision: number;
};

export type SystemsRuntimeObservability = {
  health: SystemsObservabilityHealth;
  streams: SystemsObservabilityStreams;
  os: {
    versionId: string | null;
    buildId: string | null;
    prettyName: string | null;
    activeRoot: string | null;
  };
  resourceGuard: {
    enabled: boolean;
    pressureActive: boolean;
    degradedStreamCount: number;
    recentActionCount: number;
    lastMemAvailableKb: number | null;
  };
  logSourceCount: number;
  logSourcesFreshness: SystemsLogSourcesFreshness;
  logSourcesRevision: number;
};

export type SystemsRuntimeSnapshot = {
  platform: SystemsPlatformIdentity;
  capabilities: SystemsCapabilitySnapshot;
  policies: SystemsRuntimePolicies;
  observability: SystemsRuntimeObservability;
};

export type I2cBus = {
  bus: number;
  adapter: string;
  label: string;
  path: string;
  errorCount?: number | null;
  lastError?: string | null;
};

export type I2cDevice = {
  bus: number;
  address: string;
  driver?: string | null;
  modalias?: string | null;
  kind?: string | null;
  name?: string | null;
  path: string;
};

export type I2cInventory = {
  buses: I2cBus[];
  devices: I2cDevice[];
};

export type SystemsPageErrors = {
  logs?: string | null;
  runtime?: string | null;
  i2c?: string | null;
  imu?: string | null;
};

export type SystemsPageData = {
  summary: SummaryTile[];
  device: DeviceOverview | null;
  interfaces: NetworkInterface[];
  sessions: CaptureSessionSummary[];
  rig: RigLayout;
  logs: LogStreamEntry[];
  runtime: SystemsRuntimeSnapshot;
  i2cInventory: I2cInventory;
  imu: ImuStatus;
  fetchedAt: number;
  errorMessage?: string | null;
  errors?: SystemsPageErrors;
};
