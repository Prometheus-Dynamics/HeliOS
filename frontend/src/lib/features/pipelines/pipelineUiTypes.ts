import type { PipelineDataType, PipelineNodeValue, PipelinePortMetadata } from '$lib/types/pipeline';

export type PipelineUiNodeDescriptor = {
  nodeId: string;
  backendId?: string | null;
  sourceId?: string | null;
  nodeLabel: string;
  portKey: string;
  dataType: PipelineDataType | null;
  defaultValue: PipelineNodeValue | null;
  overrideValue: PipelineNodeValue | null;
  metadata: PipelinePortMetadata | null;
};

export type PipelineUiRangeBind = { min: string; max: string };
export type PipelineUiHsvBind = { h: string; s: string; v: string };
export type PipelineUiHsvRangeBind = { h: PipelineUiRangeBind; s: PipelineUiRangeBind; v: PipelineUiRangeBind };
export type PipelineUiControlBind = string | PipelineUiRangeBind | PipelineUiHsvBind | PipelineUiHsvRangeBind;

export type PipelineUiLayoutDefinition = {
  rows: number;
  columns: number;
  outputKeys: Record<string, string | null>;
};

export type PipelineUiControl = {
  type: 'slider' | 'dual_slider' | 'toggle' | 'select' | 'input' | 'color' | 'hsv' | 'hsv_range' | 'layout_toggle';
  id: string;
  label: string;
  bind?: PipelineUiControlBind;
  min?: number;
  max?: number;
  step?: number;
  options?: ReadonlyArray<string>;
  default?: string | number | boolean | readonly [number, number] | null;
  hsvDefaults?: { h: number; s: number; v: number };
  hsvRangeDefaults?: { h: readonly [number, number]; s: readonly [number, number]; v: readonly [number, number] };
  hsvRangeMode?: 'include' | 'exclude';
  trackGradient?: string;
  trackFill?: string;
  thumbFill?: string;
  thumbBorder?: string;
  thumbBorderWidth?: number;
  help?: string;
  layout?: PipelineUiLayoutDefinition;
};

export type PipelineUiTitle = {
  type: 'title';
  text: string;
  id?: string;
};

export type PipelineUiText = {
  type: 'text';
  text: string;
  id?: string;
};

export type PipelineUiDivider = {
  type: 'divider';
  id: string;
  label?: string;
};

export type PipelineUiGroup = {
  type: 'group';
  id: string;
  title: string;
  description?: string;
  items: ReadonlyArray<PipelineUiItem>;
};

export type PipelineUiAccordion = {
  type: 'accordion';
  id: string;
  title: string;
  description?: string;
  defaultOpen?: boolean;
  items: ReadonlyArray<PipelineUiItem>;
};

export type PipelineUiTab = {
  id: string;
  title: string;
  content: ReadonlyArray<PipelineUiItem>;
};

export type PipelineUiTabsItem = {
  type: 'tabs';
  id: string;
  title?: string;
  description?: string;
  tabs: ReadonlyArray<PipelineUiTab>;
};

export type PipelineUiStackItem = {
  type: 'stack';
  id: string;
  title?: string;
  description?: string;
  items: ReadonlyArray<PipelineUiItem>;
};

export type PipelineUiItem =
  | PipelineUiTitle
  | PipelineUiText
  | PipelineUiDivider
  | PipelineUiGroup
  | PipelineUiAccordion
  | PipelineUiControl
  | PipelineUiTabsItem
  | PipelineUiStackItem;

export type PipelineUiLayout = {
  type: 'tabs';
  tabs: ReadonlyArray<PipelineUiTab>;
} | {
  type: 'stack';
  items: ReadonlyArray<PipelineUiItem>;
};

export type PipelineUi = {
  title: string;
  description?: string;
  layout?: PipelineUiLayout;
  groups?: ReadonlyArray<Omit<PipelineUiGroup, 'type'> & { controls?: ReadonlyArray<PipelineUiControl> }>;
};

export const DEFAULT_PIPELINE_UI: PipelineUi = {
  title: 'Pipeline UI',
  description: 'Curated tuning controls.',
  layout: {
    type: 'stack',
    items: []
  }
};
