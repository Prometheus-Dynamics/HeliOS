import type { PipelineGraphPlan, PipelineGraphNode, PipelineNodeLayout } from '$lib/types/pipeline';

export interface OrganizePipelineOptions {
  horizontalGap?: number;
  verticalGap?: number;
  nodeLayout?: PipelineNodeLayout | null;
}

type NodeId = string;

const defaultOptions: Required<Omit<OrganizePipelineOptions, 'nodeLayout'>> = {
  horizontalGap: 320,
  verticalGap: 160
};

const MIN_ESTIMATED_NODE_HEIGHT = 160;
const ESTIMATED_NODE_BASE_HEIGHT = 180;
const ESTIMATED_PORT_ROW_HEIGHT = 40;
const MIN_ESTIMATED_NODE_WIDTH = 320;
const ESTIMATED_NODE_BASE_WIDTH = 380;
const ESTIMATED_PORT_CHAR_WIDTH = 12;
const MAX_ESTIMATED_NODE_WIDTH = 960;

const normalizeHeight = (value: unknown): number | null => {
  if (typeof value !== 'number') return null;
  if (!Number.isFinite(value)) return null;
  if (value <= 0) return null;
  return value;
};

const estimateNodeHeight = (node: PipelineGraphNode | undefined): number => {
  const portCount = Math.max(
    Object.keys(node?.inputs ?? {}).length,
    Object.keys(node?.outputs ?? {}).length
  );
  const estimated = ESTIMATED_NODE_BASE_HEIGHT + portCount * ESTIMATED_PORT_ROW_HEIGHT;
  return Math.max(estimated, MIN_ESTIMATED_NODE_HEIGHT);
};

const estimateNodeWidth = (node: PipelineGraphNode | undefined): number => {
  const names = [
    ...Object.keys(node?.inputs ?? {}),
    ...Object.keys(node?.outputs ?? {})
  ];
  const maxPortLen = names.reduce((max, name) => Math.max(max, name.length), 0);
  const headerLabel = node?.metadata?.name ?? node?.backendId ?? '';
  const headerLen = typeof headerLabel === 'string' ? headerLabel.length : 0;
  const maxLen = Math.max(maxPortLen, headerLen);
  const estimated = ESTIMATED_NODE_BASE_WIDTH + maxLen * ESTIMATED_PORT_CHAR_WIDTH;
  return Math.max(MIN_ESTIMATED_NODE_WIDTH, Math.min(MAX_ESTIMATED_NODE_WIDTH, estimated));
};

export function organizePipelineGraph(plan: PipelineGraphPlan, options: OrganizePipelineOptions = {}): PipelineGraphPlan {
  const nodes = plan?.nodes ?? {};
  const nodeIds = Object.keys(nodes);
  if (nodeIds.length === 0) {
    return plan;
  }

  const { nodeLayout } = options;
  const { horizontalGap, verticalGap } = { ...defaultOptions, ...options };
  const incoming = new Map<NodeId, Set<NodeId>>();
  const outgoing = new Map<NodeId, Set<NodeId>>();
  const layoutLookup: PipelineNodeLayout = nodeLayout ?? {};

  const getNodeHeight = (id: NodeId): number => {
    const layoutHeight = normalizeHeight(layoutLookup[id]?.height);
    if (layoutHeight != null) {
      return layoutHeight;
    }
    return estimateNodeHeight(nodes[id]);
  };

  const getNodeWidth = (id: NodeId): number => {
    const layoutWidth = normalizeHeight(layoutLookup[id]?.width);
    if (layoutWidth != null) {
      return layoutWidth;
    }
    return estimateNodeWidth(nodes[id]);
  };

  nodeIds.forEach((id) => {
    incoming.set(id, new Set());
    outgoing.set(id, new Set());
  });

  for (const connection of plan.connections ?? []) {
    const fromNode = connection.from?.node;
    const toNode = connection.to?.node;
    if (!toNode || !incoming.has(toNode)) {
      continue;
    }
    if (fromNode && outgoing.has(fromNode)) {
      outgoing.get(fromNode)?.add(toNode);
      incoming.get(toNode)?.add(fromNode);
    }
  }

  const visiting = new Set<NodeId>();
  const levelCache = new Map<NodeId, number>();

  const computeLevel = (id: NodeId): number => {
    if (levelCache.has(id)) {
      return levelCache.get(id)!;
    }
    if (visiting.has(id)) {
      return 0;
    }
    visiting.add(id);
    let maxLevel = -1;
    for (const parent of incoming.get(id) ?? []) {
      const candidate = computeLevel(parent);
      if (candidate > maxLevel) {
        maxLevel = candidate;
      }
    }
    visiting.delete(id);
    const result = maxLevel + 1;
    levelCache.set(id, result);
    return result;
  };

  nodeIds.forEach((id) => computeLevel(id));

  const uniqueLevels = Array.from(new Set(levelCache.values())).sort((a, b) => a - b);
  const levelIndexMap = new Map<number, number>();
  uniqueLevels.forEach((level, index) => levelIndexMap.set(level, index));

  const columns = new Map<number, NodeId[]>();
  nodeIds.forEach((id) => {
    const level = levelCache.get(id) ?? 0;
    const columnIndex = levelIndexMap.get(level) ?? 0;
    if (!columns.has(columnIndex)) {
      columns.set(columnIndex, []);
    }
    columns.get(columnIndex)!.push(id);
  });

  const columnEntries = Array.from(columns.entries())
    .sort((a, b) => a[0] - b[0])
    .map(([columnIndex, ids]) => {
      const sortedIds = ids
        .slice()
        .sort((a, b) => {
          const nodeA = nodes[a];
          const nodeB = nodes[b];
          const aY = nodeA?.info?.location?.y ?? 0;
          const bY = nodeB?.info?.location?.y ?? 0;
          if (aY !== bY) {
            return aY - bY;
          }
          return a.localeCompare(b);
        });
      return { columnIndex, nodeIds: sortedIds };
    });

  const nodeColumnLookup = new Map<NodeId, number>();
  columnEntries.forEach(({ columnIndex, nodeIds }) => {
    nodeIds.forEach((id) => nodeColumnLookup.set(id, columnIndex));
  });

  const initialOrderLookup = new Map<NodeId, number>();
  columnEntries.forEach(({ nodeIds }) => {
    nodeIds.forEach((id, order) => initialOrderLookup.set(id, order));
  });

  const nodeOrderLookup = new Map<NodeId, number>();
  const refreshOrderLookup = () => {
    columnEntries.forEach(({ nodeIds }) => {
      nodeIds.forEach((id, order) => nodeOrderLookup.set(id, order));
    });
  };
  refreshOrderLookup();

  const computeOrderScore = (id: NodeId, direction: 'forward' | 'backward'): number => {
    const currentColumn = nodeColumnLookup.get(id) ?? 0;
    const primaryNeighbors =
      direction === 'forward' ? Array.from(incoming.get(id) ?? []) : Array.from(outgoing.get(id) ?? []);
    const relevantPrimary = primaryNeighbors.filter((neighborId) => {
      const neighborColumn = nodeColumnLookup.get(neighborId);
      if (neighborColumn == null) {
        return false;
      }
      return direction === 'forward' ? neighborColumn < currentColumn : neighborColumn > currentColumn;
    });

    const averageOrder = (ids: NodeId[]): number | null => {
      if (ids.length === 0) {
        return null;
      }
      const total = ids.reduce((sum, neighborId) => sum + (nodeOrderLookup.get(neighborId) ?? 0), 0);
      return total / ids.length;
    };

    const primaryScore = averageOrder(relevantPrimary);
    if (primaryScore != null) {
      return primaryScore;
    }

    const secondaryNeighbors =
      direction === 'forward' ? Array.from(outgoing.get(id) ?? []) : Array.from(incoming.get(id) ?? []);
    const relevantSecondary = secondaryNeighbors.filter((neighborId) => {
      const neighborColumn = nodeColumnLookup.get(neighborId);
      if (neighborColumn == null) {
        return false;
      }
      return direction === 'forward' ? neighborColumn > currentColumn : neighborColumn < currentColumn;
    });

    const secondaryScore = averageOrder(relevantSecondary);
    if (secondaryScore != null) {
      return secondaryScore;
    }

    return initialOrderLookup.get(id) ?? 0;
  };

  const runBarycentricPass = (direction: 'forward' | 'backward'): boolean => {
    if (columnEntries.length <= 1) {
      return false;
    }
    let changed = false;
    const start = direction === 'forward' ? 1 : columnEntries.length - 2;
    const end = direction === 'forward' ? columnEntries.length : -1;
    const step = direction === 'forward' ? 1 : -1;

    for (let index = start; index !== end; index += step) {
      const column = columnEntries[index];
      if (!column) {
        continue;
      }
      const previousOrder = column.nodeIds.slice();
      const scored = column.nodeIds.map((id) => ({
        id,
        score: computeOrderScore(id, direction),
        fallback: initialOrderLookup.get(id) ?? 0
      }));
      scored.sort((a, b) => {
        if (a.score !== b.score) {
          return a.score - b.score;
        }
        if (a.fallback !== b.fallback) {
          return a.fallback - b.fallback;
        }
        return a.id.localeCompare(b.id);
      });
      column.nodeIds = scored.map((entry) => entry.id);
      if (!changed && previousOrder.some((id, idx) => id !== column.nodeIds[idx])) {
        changed = true;
      }
    }

    if (changed) {
      refreshOrderLookup();
    }
    return changed;
  };

  if (columnEntries.length > 1) {
    const PASS_COUNT = 3;
    for (let i = 0; i < PASS_COUNT; i += 1) {
      const forwardChanged = runBarycentricPass('forward');
      const backwardChanged = runBarycentricPass('backward');
      if (!forwardChanged && !backwardChanged) {
        break;
      }
    }
  }

  const columnWidths = columnEntries.map(({ nodeIds }) =>
    nodeIds.reduce((max, id) => Math.max(max, getNodeWidth(id)), 0)
  );
  const totalWidth =
    columnWidths.reduce((sum, width) => sum + width, 0) +
    Math.max(0, columnWidths.length - 1) * horizontalGap;
  let cursorX = -totalWidth / 2;
  const targetPositions = new Map<NodeId, { x: number; y: number }>();

  columnEntries.forEach(({ nodeIds }, position) => {
    const columnWidth = columnWidths[position] ?? MIN_ESTIMATED_NODE_WIDTH;
    const x = Math.round(cursorX + columnWidth / 2);
    const columnNodes = nodeIds.map((id) => ({ id, height: getNodeHeight(id) }));
    const totalHeight = columnNodes.reduce((sum, entry) => sum + entry.height, 0);
    const totalSpacing = Math.max(0, columnNodes.length - 1) * verticalGap;
    const startY = -((totalHeight + totalSpacing) / 2);
    let cursor = startY;
    columnNodes.forEach(({ id, height }) => {
      const centerY = Math.round(cursor + height / 2);
      targetPositions.set(id, { x, y: centerY });
      cursor += height + verticalGap;
    });
    cursorX += columnWidth + horizontalGap;
  });

  let changed = false;

  const updatedNodes: Record<string, PipelineGraphNode> = Object.fromEntries(
    nodeIds.map((id) => {
      const node = nodes[id];
      const target = targetPositions.get(id) ?? { x: 0, y: 0 };
      const currentLocation = node.info?.location ?? { x: 0, y: 0 };
      if (currentLocation.x !== target.x || currentLocation.y !== target.y) {
        changed = true;
        const info = node.info
          ? {
              ...node.info,
              location: { x: target.x, y: target.y }
            }
          : {
              id: node.id,
              location: { x: target.x, y: target.y }
            };
        return [
          id,
          {
            ...node,
            info
          }
        ];
      }
      return [id, node];
    })
  );

  if (!changed) {
    return plan;
  }

  return {
    ...plan,
    nodes: updatedNodes
  };
}
