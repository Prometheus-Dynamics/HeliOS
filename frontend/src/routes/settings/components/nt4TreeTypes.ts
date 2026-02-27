export type NtTreeNode = {
  kind: 'folder' | 'topic';
  name: string;
  path: string;
  topicCount: number;
  dataType?: string | null;
  children?: NtTreeNode[];
};

