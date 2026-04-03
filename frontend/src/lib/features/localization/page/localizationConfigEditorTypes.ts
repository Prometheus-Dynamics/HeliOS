import type {
  LocalizationSolverRuntimeTuningConfig
} from '$lib/features/localization/localizationConfig';
import type { LocalizationPipelineSource } from '$lib/features/localization/pipelineSources';

export type RuntimeTuningFieldKey = Extract<keyof LocalizationSolverRuntimeTuningConfig, string>;

export type SourceGroup = {
  key: string;
  kind?: 'stream' | 'profile' | 'peer' | 'peripheral';
  label: string;
  path?: string | null;
  pipelines: Array<{
    key: string;
    label: string;
    sources: LocalizationPipelineSource[];
  }>;
};

export type SourceStatusRow = {
  source: LocalizationPipelineSource;
  pollMs: number;
  detections: number;
  tagSize: number | null;
  graphMs: number | null;
  metricsUpdatedAt: number | null;
  metricsError: string | null;
  error: string | null;
};

export type RuntimeTuningFieldSpec = {
  key: RuntimeTuningFieldKey;
  label: string;
  step: string;
};

export type RuntimeTuningGroup = {
  id: string;
  label: string;
  description: string;
  fields: RuntimeTuningFieldSpec[];
};
