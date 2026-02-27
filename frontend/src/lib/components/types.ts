export type UsageTileConfig = {
  label: string;
  value: number | string;
  trend: string;
  sparkClass: string;
  valueClass: string;
  series: number[];
  max: number;
  unit: string;
  precision: number;
  tooltip?: string;
  warning?: boolean;
  warningLabel?: string;
};

export type KeyedItem = {
  id: string;
  [key: string]: unknown;
};
