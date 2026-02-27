export type SensorBenchSummary = {
  benchmark_id: string;
  backend: string;
  device_keys: string[];
  completed_at: string;
  canceled?: boolean;
};

export type SensorBenchListItem = {
  summary: SensorBenchSummary;
};

export type SensorBenchCodecStat = {
  implementation: string;
  avg_ms?: number;
  avg_fps?: number;
  errors?: number;
  cpu_delta?: { engine_cpu_avg?: number | null; system_cpu_avg?: number | null } | null;
};

export type SensorBenchModeResult = {
  format: string;
  resolution: string;
  capture_avg_fps?: number;
  host_avg_fps?: number;
  decoders?: SensorBenchCodecStat[];
  encoders?: SensorBenchCodecStat[];
  encoder_input_decoder?: string | null;
};

export type SensorBenchResult = {
  summary: SensorBenchSummary;
  modes: SensorBenchModeResult[];
  warnings?: string[];
};
