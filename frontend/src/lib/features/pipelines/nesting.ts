import type { PipelineGraphNode, PipelineGraphPlan } from '$lib/types/pipeline';

export type EditingFrame = {
  nodeId: string | null;
  plan: PipelineGraphPlan;
};

export type EditingPath = string[];

export const rootFrame = (plan: PipelineGraphPlan): EditingFrame => ({ nodeId: null, plan });

export const resolvePlanAtPath = (plan: PipelineGraphPlan, path: EditingPath): PipelineGraphPlan | null => {
  let current: PipelineGraphPlan | null = plan;
  for (const id of path) {
    if (!current) return null;
    const node: PipelineGraphNode | undefined = current.nodes?.[id];
    if (!node?.embedded) return null;
    current = node.embedded;
  }
  return current;
};

export const replacePlanAtPath = (root: PipelineGraphPlan, path: EditingPath, next: PipelineGraphPlan): PipelineGraphPlan | null => {
  if (path.length === 0) {
    return next;
  }
  const [head, ...rest] = path;
  const node = root.nodes?.[head];
  if (!node?.embedded) return null;
  const replacedChild = replacePlanAtPath(node.embedded, rest, next);
  if (!replacedChild) return null;
  const clonedNodes = { ...(root.nodes ?? {}) };
  clonedNodes[head] = { ...node, embedded: replacedChild };
  return {
    ...root,
    nodes: clonedNodes
  };
};

export const pushFrame = (stack: EditingFrame[], frame: EditingFrame): EditingFrame[] => [...stack, frame];

export const popFrame = (stack: EditingFrame[]): EditingFrame[] => {
  if (stack.length <= 1) return stack;
  return stack.slice(0, stack.length - 1);
};

export const replaceTopFrame = (stack: EditingFrame[], plan: PipelineGraphPlan): EditingFrame[] => {
  if (stack.length === 0) return [rootFrame(plan)];
  const next = stack.slice();
  next[next.length - 1] = { nodeId: stack[stack.length - 1].nodeId, plan };
  return next;
};

export const derivePath = (stack: EditingFrame[]): EditingPath =>
  stack
    .map((frame) => frame.nodeId)
    .filter((id): id is string => typeof id === 'string' && id.length > 0);

export const enterEmbedded = (
  stack: EditingFrame[],
  nodeId: string,
  parentPlan: PipelineGraphPlan
): EditingFrame[] => {
  const child = parentPlan.nodes?.[nodeId]?.embedded;
  if (!child) return stack;
  return pushFrame(stack, { nodeId, plan: child });
};
