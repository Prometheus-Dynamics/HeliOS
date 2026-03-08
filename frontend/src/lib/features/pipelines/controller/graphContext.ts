import { get, type Readable, type Writable } from 'svelte/store';
import { toaster } from '$lib';
import type {
  PipelineDataType,
  PipelineGraphPlan,
  PipelineNodeStyle,
  PipelineOverviewPipeline,
  PipelineRegistryEntry
} from '$lib/types/pipeline';
import type { PipelineGraphEdgeSelection } from '$lib';
import {
  normalizePipelinePortName,
  PIPELINE_INPUT_BACKEND_ID,
  PIPELINE_OUTPUT_BACKEND_ID
} from '../boundary';
import { normalizeNodeStyle } from '../model';
import type {
  BoundaryDraft,
  GraphContextMenuState,
  GraphContextPort,
  GroupDraft
} from './types';
import { ACTION_MENU_SIZE, BOUNDARY_MENU_SIZE, GROUP_MENU_SIZE, PANE_MENU_SIZE } from './uiSizes';

export type PipelineGraphContextDeps = {
  editingPlan: Readable<PipelineGraphPlan | null>;
  selectedPipeline: Readable<PipelineOverviewPipeline | null>;
  graphContextMenu: Writable<GraphContextMenuState>;
  graphContextSearch: Writable<string>;
  graphSelection: Writable<{
    nodeId: string | null;
    nodes: string[];
    edge: PipelineGraphEdgeSelection | { id: string } | null;
  }>;
  registryLoading: Readable<boolean>;
  registryError: Readable<string | null>;
  registry: Readable<PipelineRegistryEntry[]>;
  scheduleRegistryRefresh: () => void;
  closeGraphContextMenu: () => void;
  resolveDataTypeKey: (dataType: PipelineDataType | null | undefined) => string | null;
  generateNodeId: () => string;
  inferPortDataType: (plan: PipelineGraphPlan | null | undefined, nodeId: string, port: string, direction: 'input' | 'output') => PipelineDataType;
  resolveHostIoDirection: (node: { backendId?: string | null } | null | undefined) => 'input' | 'output' | null;
  addPipelinePort: (direction: 'input' | 'output', name: string, dataTypeKey: string, options?: { position?: { x: number; y: number } | null; select?: boolean }) => string | null;
  addHostIoPort: (nodeId: string, name: string, dataTypeKey: string) => void;
  editPipelinePort: (direction: 'input' | 'output', nodeId: string, name: string, dataTypeKey: string, oldName?: string) => string | null;
  removePipelinePort: (direction: 'input' | 'output', name: string) => void;
  setNodeMetadata: (nodeId: string, metadata: { name?: string; summary?: string; style?: PipelineNodeStyle | null }) => void;
};

export const createPipelineGraphContext = (deps: PipelineGraphContextDeps) => {
  const {
    editingPlan,
    selectedPipeline,
    graphContextMenu,
    graphContextSearch,
    graphSelection,
    registryLoading,
    registryError,
    registry,
    scheduleRegistryRefresh,
    closeGraphContextMenu,
    generateNodeId,
    inferPortDataType,
    resolveHostIoDirection,
    addPipelinePort,
    addHostIoPort,
    editPipelinePort,
    removePipelinePort,
    setNodeMetadata
  } = deps;

  const clampMenuPosition = (position: { x: number; y: number }, size: { width: number; height: number }) => {
    if (typeof window === 'undefined') {
      return position;
    }
    const padding = 12;
    const maxX = Math.max(padding, window.innerWidth - size.width - padding);
    const maxY = Math.max(padding, window.innerHeight - size.height - padding);
    return {
      x: Math.min(Math.max(padding, position.x), maxX),
      y: Math.min(Math.max(padding, position.y), maxY)
    };
  };

  const openGraphContext = (detail: {
    type: 'pane' | 'node' | 'palette' | 'port';
    position: { x: number; y: number };
    flowPosition: { x: number; y: number };
    nodeId?: string | null;
    port?: string | null;
    direction?: 'input' | 'output';
  }) => {
    const { type, position, flowPosition, nodeId, port, direction } = detail;
    const initialMode = type === 'palette' ? 'registry' : 'actions';
    const size = initialMode === 'registry' ? PANE_MENU_SIZE : ACTION_MENU_SIZE;
    const clamped = clampMenuPosition(position, size);
    const plan = get(editingPlan);
    const portInfo: GraphContextPort | null =
      type === 'port' && nodeId && port && direction
        ? {
            nodeId,
            port: normalizePipelinePortName(port),
            direction,
            dataType: inferPortDataType(plan, nodeId, port, direction)
          }
        : null;
    graphContextMenu.set({
      visible: true,
      mode: initialMode,
      position: clamped,
      flowPosition,
      nodeId: nodeId ?? null,
      size,
      boundary: null,
      group: null,
      port: portInfo
    });
    graphContextSearch.set('');
    const current = get(graphSelection);
    const existingNodes = Array.isArray(current.nodes) ? current.nodes.filter(Boolean) : [];
    const nodeList =
      (type === 'node' || type === 'port') && nodeId
        ? existingNodes.includes(nodeId)
          ? existingNodes
          : [...existingNodes, nodeId]
        : existingNodes;
    const nextNodeId = type === 'node' || type === 'port' ? nodeId ?? current.nodeId ?? null : current.nodeId ?? null;
    graphSelection.set({ nodeId: nextNodeId, nodes: nodeList, edge: null });
  };

  const openRegistryPalette = () => {
    const menu = get(graphContextMenu);
    if (!menu.visible) return;
    if (!get(registryLoading) && (get(registryError) || get(registry).length === 0)) {
      scheduleRegistryRefresh();
    }
    const visibleMenu = menu as Extract<GraphContextMenuState, { visible: true }>;
    const size = PANE_MENU_SIZE;
    const clamped = clampMenuPosition(visibleMenu.position, size);
    graphContextMenu.set({
      visible: true,
      mode: 'registry',
      position: clamped,
      flowPosition: visibleMenu.flowPosition,
      nodeId: visibleMenu.nodeId,
      size,
      boundary: null,
      group: null,
      port: visibleMenu.port ?? null
    });
    graphContextSearch.set('');
  };

  const openActionMenu = () => {
    const menu = get(graphContextMenu);
    if (!menu.visible) return;
    const visibleMenu = menu as Extract<GraphContextMenuState, { visible: true }>;
    const size = ACTION_MENU_SIZE;
    const clamped = clampMenuPosition(visibleMenu.position, size);
    graphContextMenu.set({
      visible: true,
      mode: 'actions',
      position: clamped,
      flowPosition: visibleMenu.flowPosition,
      nodeId: visibleMenu.nodeId,
      size,
      boundary: null,
      group: null,
      port: visibleMenu.port ?? null
    });
    graphContextSearch.set('');
  };

  const openBoundaryMenu = (params?: {
    direction?: 'input' | 'output';
    nodeId?: string | null;
    ports?: Array<{ name: string; dataTypeKey?: string; originalName?: string | null }>;
    name?: string;
    dataTypeKey?: string;
  }) => {
    const menu = get(graphContextMenu);
    if (!menu.visible) return;
    const visibleMenu = menu as Extract<GraphContextMenuState, { visible: true }>;
    const size = BOUNDARY_MENU_SIZE;
    const clamped = clampMenuPosition(visibleMenu.position, size);
    const portDefaults = visibleMenu.port ?? null;
    const defaultDirection = params?.direction ?? portDefaults?.direction ?? 'input';
    const defaultName = params?.name ?? portDefaults?.port ?? '';
    const defaultTypeKey = 'generic';
    const ports =
      params?.ports && params.ports.length > 0
        ? params.ports.map((entry) => ({
            id: generateNodeId(),
            name: entry.name ?? '',
            dataTypeKey: entry.dataTypeKey?.trim() || 'generic',
            originalName: entry.originalName ?? entry.name ?? null
          }))
        : [
            {
              id: generateNodeId(),
              name: defaultName,
              dataTypeKey: defaultTypeKey,
              originalName: params?.nodeId ? params?.name ?? portDefaults?.port ?? null : null
            }
          ];
    const boundary: BoundaryDraft = {
      direction: defaultDirection,
      nodeId: params?.nodeId ?? null,
      ports
    };
    graphContextMenu.set({
      visible: true,
      mode: 'boundary',
      position: clamped,
      flowPosition: visibleMenu.flowPosition,
      nodeId: visibleMenu.nodeId,
      size,
      boundary,
      group: null,
      port: visibleMenu.port ?? null
    });
    graphContextSearch.set('');
  };

  const setBoundaryDraftPortName = (portId: string, name: string) => {
    const menu = get(graphContextMenu);
    if (!menu.visible || menu.mode !== 'boundary' || !menu.boundary) return;
    const ports = menu.boundary.ports.map((port) => (port.id === portId ? { ...port, name } : port));
    graphContextMenu.set({ ...menu, boundary: { ...menu.boundary, ports } });
  };

  const setBoundaryDraftPortType = (portId: string, dataTypeKey: string) => {
    const menu = get(graphContextMenu);
    if (!menu.visible || menu.mode !== 'boundary' || !menu.boundary) return;
    const ports = menu.boundary.ports.map((port) =>
      port.id === portId ? { ...port, dataTypeKey } : port
    );
    graphContextMenu.set({ ...menu, boundary: { ...menu.boundary, ports } });
  };

  const addBoundaryDraftPort = () => {
    const menu = get(graphContextMenu);
    if (!menu.visible || menu.mode !== 'boundary' || !menu.boundary) return;
    const ports = [
      ...menu.boundary.ports,
      { id: generateNodeId(), name: '', dataTypeKey: 'generic', originalName: null }
    ];
    graphContextMenu.set({ ...menu, boundary: { ...menu.boundary, ports } });
  };

  const removeBoundaryDraftPort = (portId: string) => {
    const menu = get(graphContextMenu);
    if (!menu.visible || menu.mode !== 'boundary' || !menu.boundary) return;
    const ports = menu.boundary.ports.filter((port) => port.id !== portId);
    graphContextMenu.set({ ...menu, boundary: { ...menu.boundary, ports } });
  };

  const setBoundaryDraftDirection = (direction: 'input' | 'output') => {
    const menu = get(graphContextMenu);
    if (!menu.visible || menu.mode !== 'boundary' || !menu.boundary) return;
    if (menu.boundary.nodeId) return;
    graphContextMenu.set({ ...menu, boundary: { ...menu.boundary, direction } });
  };

  const openBoundaryEditor = (nodeId: string | null) => {
    const menu = get(graphContextMenu);
    const plan = get(editingPlan);
    if (!menu.visible || !plan || !nodeId) return;
    const node = plan.nodes?.[nodeId];
    if (!node) return;
    const backendId = (node.backendId ?? '').toLowerCase();
    const direction =
      backendId === PIPELINE_INPUT_BACKEND_ID
        ? 'input'
        : backendId === PIPELINE_OUTPUT_BACKEND_ID
          ? 'output'
          : null;
    if (!direction) return;
    const portRecord = direction === 'input' ? node.outputs ?? {} : node.inputs ?? {};
    const boundaryRecord = direction === 'input' ? plan.pipelineInputs ?? {} : plan.pipelineOutputs ?? {};
    const portNames = new Set<string>([...Object.keys(portRecord), ...Object.keys(boundaryRecord)]);
    const ports =
      portNames.size > 0
        ? Array.from(portNames).map((portName) => {
            return {
              name: portName,
              originalName: portName,
              dataTypeKey: 'generic'
            };
          })
        : [
            {
              name: '',
              originalName: null,
              dataTypeKey: 'generic'
            }
          ];
    openBoundaryMenu({
      direction,
      nodeId,
      ports
    });
  };

  const applyBoundaryDraft = () => {
    const menu = get(graphContextMenu);
    if (!menu.visible || menu.mode !== 'boundary' || !menu.boundary) return;
    const draft = menu.boundary;
    const pipeline = get(selectedPipeline);
    const plan = get(editingPlan);
    if (!pipeline || !plan) return;
    const normalizedDraftPorts = draft.ports.map((port) => ({
      ...port,
      name: port.name.trim(),
      dataTypeKey: 'generic'
    }));
    const draftWithNames = normalizedDraftPorts.filter((port) => Boolean(port.name));
    const invalidExisting = normalizedDraftPorts.some((port) => !port.name && port.originalName);
    if (invalidExisting) {
      toaster.error({ title: 'Port name required', description: 'Remove empty entries or provide a name.' });
      return;
    }
    const seenNames = new Set<string>();
    const duplicate = draftWithNames.find((port) => {
      const normalized = normalizePipelinePortName(port.name);
      if (seenNames.has(normalized)) {
        return true;
      }
      seenNames.add(normalized);
      return false;
    });
    if (duplicate) {
      toaster.error({
        title: 'Duplicate port',
        description: `Port "${duplicate.name}" appears multiple times. Use unique names.`
      });
      return;
    }

    if (!draft.nodeId) {
      if (draftWithNames.length === 0) {
        toaster.error({ title: 'Port name required', description: 'Add at least one pipeline port.' });
        return;
      }
      const boundaryRecord = draft.direction === 'input' ? plan.pipelineInputs ?? {} : plan.pipelineOutputs ?? {};
      const conflict = draftWithNames.find((port) => boundaryRecord[normalizePipelinePortName(port.name)]);
      if (conflict) {
        toaster.error({
          title: 'Port already exists',
          description: `Pipeline ${draft.direction} "${normalizePipelinePortName(conflict.name)}" is already defined.`
        });
        return;
      }
      let createdAny = false;
      draftWithNames.forEach((port, index) => {
        const created = addPipelinePort(draft.direction, port.name, 'generic', {
          position: index === 0 ? menu.flowPosition : null,
          select: index === 0
        });
        if (created) {
          createdAny = true;
        }
      });
      if (createdAny) {
        closeGraphContextMenu();
      }
      return;
    }

    const nodeId = draft.nodeId.trim();
    const node = plan.nodes?.[nodeId];
    if (!node) {
      toaster.error({ title: 'Pipeline IO not found', description: 'Selected pipeline IO node could not be located.' });
      return;
    }
    const nodeDirection = resolveHostIoDirection(node);
    if (!nodeDirection || nodeDirection !== draft.direction) {
      toaster.error({ title: 'Unsupported node', description: 'Only pipeline input/output nodes can be edited here.' });
      return;
    }
    const portRecord = draft.direction === 'input' ? node.outputs ?? {} : node.inputs ?? {};
    const existingPorts = Object.keys(portRecord);
    const existingKeyByNormalized = new Map<string, string>();
    existingPorts.forEach((port) => {
      const normalized = normalizePipelinePortName(port);
      if (normalized) {
        existingKeyByNormalized.set(normalized, port);
      }
    });
    const existingTypeByPort = Object.keys(
      draft.direction === 'input' ? plan.pipelineInputs ?? {} : plan.pipelineOutputs ?? {}
    ).reduce<Record<string, string>>((acc, key) => {
      acc[normalizePipelinePortName(key)] = 'generic';
      return acc;
    }, {});
    const normalizedDraftEntries = draftWithNames.map((port) => {
      const normalizedNext = normalizePipelinePortName(port.name);
      let normalizedOriginal = normalizePipelinePortName(port.originalName ?? '');
      let resolvedOriginalName = port.originalName ?? null;
      if (!normalizedOriginal && normalizedNext && existingKeyByNormalized.has(normalizedNext)) {
        normalizedOriginal = normalizedNext;
        resolvedOriginalName = existingKeyByNormalized.get(normalizedNext) ?? port.name;
      }
      return {
        ...port,
        normalizedNext,
        normalizedOriginal,
        resolvedOriginalName
      };
    });
    const originalPortSet = new Set(
      normalizedDraftEntries.map((port) => port.normalizedOriginal).filter(Boolean)
    );

    const newPorts = normalizedDraftEntries.filter((port) => !port.normalizedOriginal);
    newPorts.forEach((port) => {
      addHostIoPort(nodeId, port.name, 'generic');
    });

    existingPorts.forEach((port) => {
      const normalized = normalizePipelinePortName(port);
      if (!originalPortSet.has(normalized)) {
        removePipelinePort(draft.direction, port);
      }
    });

    normalizedDraftEntries.forEach((port) => {
      if (!port.normalizedOriginal) return;
      const normalizedOriginal = port.normalizedOriginal;
      const normalizedNext = port.normalizedNext;
      const needsRename = normalizedOriginal !== normalizedNext;
      const existingType = existingTypeByPort[normalizedOriginal] ?? 'generic';
      const needsTypeChange = port.dataTypeKey.trim() && port.dataTypeKey.trim() !== existingType;
      if (needsRename || needsTypeChange) {
        editPipelinePort(draft.direction, nodeId, port.name, 'generic', port.resolvedOriginalName ?? undefined);
      }
    });

    closeGraphContextMenu();
  };

  const openGroupEditor = (nodeId: string | null) => {
    const menu = get(graphContextMenu);
    const plan = get(editingPlan);
    if (!menu.visible || !plan || !nodeId) return;
    const visibleMenu = menu as Extract<GraphContextMenuState, { visible: true }>;
    const node = plan.nodes?.[nodeId];
    if (!node || (node.backendId ?? '').toLowerCase() !== 'pipeline:child') return;
    const size = GROUP_MENU_SIZE;
    const clamped = clampMenuPosition(visibleMenu.position, size);
    const normalizedStyle = normalizeNodeStyle(node.metadata?.style);
    const color =
      (normalizedStyle?.bg_color?.trim() ?? '') || (normalizedStyle?.border_color?.trim() ?? '') || '';
    const group: GroupDraft = {
      nodeId,
      name: node.metadata?.name ?? '',
      summary: node.metadata?.summary ?? '',
      color
    };
    graphContextMenu.set({
      visible: true,
      mode: 'group',
      position: clamped,
      flowPosition: visibleMenu.flowPosition,
      nodeId: visibleMenu.nodeId,
      size,
      boundary: null,
      group,
      port: visibleMenu.port ?? null
    });
    graphContextSearch.set('');
  };

  const setGroupDraftName = (name: string) => {
    const menu = get(graphContextMenu);
    if (!menu.visible || menu.mode !== 'group' || !menu.group) return;
    graphContextMenu.set({ ...menu, group: { ...menu.group, name } });
  };

  const setGroupDraftSummary = (summary: string) => {
    const menu = get(graphContextMenu);
    if (!menu.visible || menu.mode !== 'group' || !menu.group) return;
    graphContextMenu.set({ ...menu, group: { ...menu.group, summary } });
  };

  const setGroupDraftColor = (color: string) => {
    const menu = get(graphContextMenu);
    if (!menu.visible || menu.mode !== 'group' || !menu.group) return;
    graphContextMenu.set({ ...menu, group: { ...menu.group, color } });
  };

  const applyGroupDraft = () => {
    const menu = get(graphContextMenu);
    if (!menu.visible || menu.mode !== 'group' || !menu.group) return;
    const draft = menu.group;
    const name = draft.name?.trim() ?? '';
    if (!name) return;
    const summary = draft.summary?.trim() ?? '';
    const color = draft.color?.trim() ?? '';
    const style: PipelineNodeStyle | null = color ? { bg_color: color, border_color: color } : null;
    setNodeMetadata(draft.nodeId, {
      name,
      summary,
      style
    });
    closeGraphContextMenu();
  };

  return {
    clampMenuPosition,
    openGraphContext,
    closeGraphContextMenu,
    openRegistryPalette,
    openActionMenu,
    openBoundaryMenu,
    setBoundaryDraftPortName,
    setBoundaryDraftPortType,
    addBoundaryDraftPort,
    removeBoundaryDraftPort,
    setBoundaryDraftDirection,
    openBoundaryEditor,
    applyBoundaryDraft,
    openGroupEditor,
    setGroupDraftName,
    setGroupDraftSummary,
    setGroupDraftColor,
    applyGroupDraft
  };
};
