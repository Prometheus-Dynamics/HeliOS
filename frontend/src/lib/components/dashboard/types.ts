export type StreamSegment = {
  label: string;
  value: number;
  color: string;
  detail: string;
};

export type TimelineItem = {
  time: string;
  title: string;
  detail?: string;
  id?: string;
};
