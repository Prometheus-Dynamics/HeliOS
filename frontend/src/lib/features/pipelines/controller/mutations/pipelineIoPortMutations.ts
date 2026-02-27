import { get } from 'svelte/store';
import type {
  PipelineConnection,
  PipelineDataType,
  PipelineGraphNode,
  PipelineNodeValue
} from '$lib/types/pipeline';
import { toaster } from '$lib';
import { normalizeImageFlavorType } from '$lib/features/pipelines/valueFormatting';
import {
  normalizePipelinePortName,
  PIPELINE_INPUT_BACKEND_ID,
  PIPELINE_OUTPUT_BACKEND_ID,
  refreshPipelineIoCaches,
  removePipelineBoundaryNode
} from '../../boundary';
import type { PipelineMutationsDeps } from './types';

export const createPipelineIoPortMutations = (deps: PipelineMutationsDeps) => {
  const {
    selectedPipeline,
    editingPlan,
    graphSelection,
    updateCurrentPlan,
    closeGraphContextMenu,
    cloneDataType,
    buildDataTypeFromKey,
    resolveDataTypeKey,
    findHostIoNodeId,
    resolveHostIoDirection,
    ensureHostIoNodeShape,
    generateNodeId
  } = deps;

  function addPipelinePort(
    direction: 'input' | 'output',
    name: string,
    dataTypeKey: string,
    options?: { position?: { x: number; y: number } | null; select?: boolean }
  ): string | null {
    const pipeline = get(selectedPipeline);
    if (!pipeline) return null;
    const rawName = name.trim();
    const typeKey = dataTypeKey.trim() || 'generic';
    if (!rawName) {
      toaster.error({ title: 'Port name required', description: 'Enter a unique name for the pipeline port.' });
      return null;
    }
    const portName = normalizePipelinePortName(rawName);
    const inputRecord = pipeline.graph.pipelineInputs ?? {};
    const outputRecord = pipeline.graph.pipelineOutputs ?? {};
    const exists = direction === 'input' ? Boolean(inputRecord[portName]) : Boolean(outputRecord[portName]);
    if (exists) {
      toaster.error({
        title: 'Port already exists',
        description: `Pipeline ${direction === 'input' ? 'input' : 'output'} "${portName}" is already defined.`
      });
      return null;
    }
    const dataType = buildDataTypeFromKey(typeKey);
    const clonedType = cloneDataType(dataType);
    const existingHostId = findHostIoNodeId(pipeline.graph, direction);
    if (existingHostId) {
      addHostIoPort(existingHostId, portName, typeKey);
      if (options?.select) {
        graphSelection.set({ nodeId: existingHostId, nodes: [existingHostId], edge: null });
      }
      return existingHostId;
    }
    const existingCount =
      direction === 'input'
        ? Object.keys(pipeline.graph.pipelineInputs ?? {}).length
        : Object.keys(pipeline.graph.pipelineOutputs ?? {}).length;
    const baseLocation =
      options?.position && Number.isFinite(options.position.x) && Number.isFinite(options.position.y)
        ? {
            x: options.position.x + (direction === 'input' ? -180 : 160),
            y: options.position.y
          }
        : direction === 'input'
          ? { x: -320, y: existingCount * 120 }
          : { x: 320, y: existingCount * 120 };
    const location = baseLocation;
    let createdNodeId: string | null = null;

    updateCurrentPlan((plan) => {
      plan.nodes ??= {};
      const nodeId = generateNodeId();
      const portRecord = { [portName]: clonedType };
      const node: PipelineGraphNode = {
        id: nodeId,
        backendId: direction === 'input' ? PIPELINE_INPUT_BACKEND_ID : PIPELINE_OUTPUT_BACKEND_ID,
        metadata: {
          name: direction === 'input' ? 'Pipeline Input' : 'Pipeline Output',
          provider: 'pipeline.io'
        },
        inputs: direction === 'output' ? portRecord : {},
        outputs: direction === 'input' ? portRecord : {},
        info: {
          id: direction === 'input' ? PIPELINE_INPUT_BACKEND_ID : PIPELINE_OUTPUT_BACKEND_ID,
          location
        },
        embedded: null,
        source: null
      };
      createdNodeId = nodeId;
      plan.nodes[nodeId] = node;
      if (direction === 'input') {
        const inputs = { ...(plan.pipelineInputs ?? {}) };
        inputs[portName] = clonedType;
        plan.pipelineInputs = inputs;
        const configs = { ...(plan.pipelineInputConfigs ?? {}) };
        configs[portName] = configs[portName] ?? { policy: 'NewestWins', capacity: 3 };
        plan.pipelineInputConfigs = configs;
      } else {
        const outputs = { ...(plan.pipelineOutputs ?? {}) };
        outputs[portName] = clonedType;
        plan.pipelineOutputs = outputs;
        const configs = { ...(plan.pipelineOutputConfigs ?? {}) };
        configs[portName] = configs[portName] ?? { capacity: 4 };
        plan.pipelineOutputConfigs = configs;
      }
    });
    if (options?.select && createdNodeId) {
      graphSelection.set({ nodeId: createdNodeId, nodes: [createdNodeId], edge: null });
    }
    return createdNodeId;
  }

  function addHostIoPort(nodeId: string, name: string, dataTypeKey: string): void {
    const pipeline = get(selectedPipeline);
    const plan = get(editingPlan);
    if (!pipeline || !plan) return;
    const rawName = name.trim();
    if (!rawName) return;
    const normalizedPort = normalizePipelinePortName(rawName);
    if (!normalizedPort) return;
    const node = plan.nodes?.[nodeId];
    if (!node) return;
    const direction = resolveHostIoDirection(node);
    if (!direction) return;
    if (direction === 'input' && plan.pipelineInputs?.[normalizedPort]) {
      toaster.error({
        title: 'Port already exists',
        description: `Pipeline input "${normalizedPort}" is already defined.`
      });
      return;
    }
    if (direction === 'output' && plan.pipelineOutputs?.[normalizedPort]) {
      toaster.error({
        title: 'Port already exists',
        description: `Pipeline output "${normalizedPort}" is already defined.`
      });
      return;
    }
    const existingRecord = direction === 'input' ? node.outputs ?? {} : node.inputs ?? {};
    if (Object.keys(existingRecord).some((port) => normalizePipelinePortName(port) === normalizedPort)) {
      toaster.error({
        title: 'Port already exists',
        description: `IO port "${normalizedPort}" is already defined on this node.`
      });
      return;
    }
    const dataType = cloneDataType(buildDataTypeFromKey(dataTypeKey));
    updateCurrentPlan((mutablePlan) => {
      const target = mutablePlan.nodes?.[nodeId];
      if (!target) return;
      ensureHostIoNodeShape(target, direction);
      const record = direction === 'input' ? { ...(target.outputs ?? {}) } : { ...(target.inputs ?? {}) };
      record[normalizedPort] = dataType;
      if (direction === 'input') {
        target.outputs = record;
        target.inputs = target.inputs ?? {};
        const inputs = { ...(mutablePlan.pipelineInputs ?? {}) };
        inputs[normalizedPort] = dataType;
        mutablePlan.pipelineInputs = inputs;
        const configs = { ...(mutablePlan.pipelineInputConfigs ?? {}) };
        configs[normalizedPort] = configs[normalizedPort] ?? { policy: 'NewestWins', capacity: 3 };
        mutablePlan.pipelineInputConfigs = configs;
      } else {
        target.inputs = record;
        target.outputs = target.outputs ?? {};
        const outputs = { ...(mutablePlan.pipelineOutputs ?? {}) };
        outputs[normalizedPort] = dataType;
        mutablePlan.pipelineOutputs = outputs;
        const configs = { ...(mutablePlan.pipelineOutputConfigs ?? {}) };
        configs[normalizedPort] = configs[normalizedPort] ?? { capacity: 4 };
        mutablePlan.pipelineOutputConfigs = configs;
      }
    });
  }

  function removeHostIoPort(nodeId: string, name: string): void {
    const pipeline = get(selectedPipeline);
    const plan = get(editingPlan);
    if (!pipeline || !plan) return;
    const normalizedPort = normalizePipelinePortName(name);
    if (!normalizedPort) return;
    const node = plan.nodes?.[nodeId];
    if (!node) return;
    const direction = resolveHostIoDirection(node);
    if (!direction) return;
    updateCurrentPlan((mutablePlan) => {
      const target = mutablePlan.nodes?.[nodeId];
      if (!target) return;
      const record = direction === 'input' ? { ...(target.outputs ?? {}) } : { ...(target.inputs ?? {}) };
      Object.keys(record).forEach((key) => {
        if (normalizePipelinePortName(key) === normalizedPort) {
          delete record[key];
        }
      });
      if (direction === 'input') {
        target.outputs = record;
      } else {
        target.inputs = record;
      }

      mutablePlan.connections = (mutablePlan.connections ?? []).filter((connection) => {
        const fromMatch =
          connection.from.node === nodeId && normalizePipelinePortName(connection.from.port) === normalizedPort;
        const toMatch = connection.to.node === nodeId && normalizePipelinePortName(connection.to.port) === normalizedPort;
        return !(fromMatch || toMatch);
      });

      if (direction === 'input') {
        if (mutablePlan.pipelineInputs) {
          delete mutablePlan.pipelineInputs[normalizedPort];
          if (Object.keys(mutablePlan.pipelineInputs).length === 0) {
            delete mutablePlan.pipelineInputs;
          }
        }
        if (mutablePlan.pipelineInputConfigs) {
          delete mutablePlan.pipelineInputConfigs[normalizedPort];
          if (Object.keys(mutablePlan.pipelineInputConfigs).length === 0) {
            delete mutablePlan.pipelineInputConfigs;
          }
        }
        if (mutablePlan.pipelineInputValues) {
          delete mutablePlan.pipelineInputValues[normalizedPort];
          if (Object.keys(mutablePlan.pipelineInputValues).length === 0) {
            delete mutablePlan.pipelineInputValues;
          }
        }
      } else if (direction === 'output') {
        if (mutablePlan.pipelineOutputs) {
          delete mutablePlan.pipelineOutputs[normalizedPort];
          if (Object.keys(mutablePlan.pipelineOutputs).length === 0) {
            delete mutablePlan.pipelineOutputs;
          }
        }
        if (mutablePlan.pipelineOutputConfigs) {
          delete mutablePlan.pipelineOutputConfigs[normalizedPort];
          if (Object.keys(mutablePlan.pipelineOutputConfigs).length === 0) {
            delete mutablePlan.pipelineOutputConfigs;
          }
        }
      }

      refreshPipelineIoCaches(mutablePlan);
    });
  }

  function createBoundaryFromPort(payload?: {
    nodeId?: string;
    port?: string;
    direction?: 'input' | 'output';
    dataType?: PipelineDataType | null;
    position?: { x: number; y: number } | null;
    typeLabel?: string | null;
  }): string | null {
    const pipeline = get(selectedPipeline);
    const plan = get(editingPlan);
    if (!pipeline || !plan) return null;
    const nodeId = payload?.nodeId?.trim() ?? '';
    if (!nodeId) return null;
    const portName = payload?.port?.trim() ?? '';
    if (!portName) return null;
    const direction = payload?.direction ?? 'input';
    const normalizedPort = normalizePipelinePortName(portName);
    if (!normalizedPort) return null;

    const inferredType = normalizeImageFlavorType(cloneDataType(payload?.dataType ?? 'Generic'));
    const typeLabel = payload?.typeLabel ?? resolveDataTypeKey(inferredType) ?? 'Generic';
    const anchorPosition = payload?.position ?? null;
    const offset = direction === 'input' ? -140 : 140;
    const baseLocation = anchorPosition
      ? { x: anchorPosition.x + offset, y: anchorPosition.y }
      : direction === 'input'
        ? { x: -320, y: 0 }
        : { x: 320, y: 0 };

    let createdNodeId: string | null = null;

    updateCurrentPlan((mutablePlan) => {
      const boundaryRecord = direction === 'input' ? mutablePlan.pipelineInputs ?? {} : mutablePlan.pipelineOutputs ?? {};
      if (boundaryRecord[normalizedPort]) {
        toaster.error({
          title: 'Port already exists',
          description: `Pipeline ${direction} "${normalizedPort}" is already defined.`
        });
        closeGraphContextMenu();
        return createdNodeId;
      }

      const record =
        direction === 'input' ? (mutablePlan.pipelineInputs ??= {}) : (mutablePlan.pipelineOutputs ??= {});
      record[normalizedPort] = inferredType ?? 'Generic';

      if (direction === 'input') {
        const configs = { ...(mutablePlan.pipelineInputConfigs ?? {}) };
        configs[normalizedPort] = configs[normalizedPort] ?? { policy: 'NewestWins', capacity: 3 };
        mutablePlan.pipelineInputConfigs = configs;
      } else {
        const configs = { ...(mutablePlan.pipelineOutputConfigs ?? {}) };
        configs[normalizedPort] = configs[normalizedPort] ?? { capacity: 4 };
        mutablePlan.pipelineOutputConfigs = configs;
      }

      const existingCount =
        direction === 'input'
          ? Math.max(0, Object.keys(record).length - 1)
          : Math.max(0, Object.keys(record).length - 1);
      const location =
        anchorPosition
          ? { x: anchorPosition.x + offset, y: anchorPosition.y }
          : direction === 'input'
            ? { x: -320, y: existingCount * 120 }
            : { x: 320, y: existingCount * 120 };

      const ioNodeId = generateNodeId();
      const portRecord = { [normalizedPort]: inferredType ?? 'Generic' };
      const ioNode: PipelineGraphNode = {
        id: ioNodeId,
        backendId: direction === 'input' ? PIPELINE_INPUT_BACKEND_ID : PIPELINE_OUTPUT_BACKEND_ID,
        metadata: {
          name: direction === 'input' ? 'Pipeline Input' : 'Pipeline Output',
          provider: 'pipeline.io'
        },
        inputs: direction === 'output' ? portRecord : {},
        outputs: direction === 'input' ? portRecord : {},
        info: {
          id: direction === 'input' ? PIPELINE_INPUT_BACKEND_ID : PIPELINE_OUTPUT_BACKEND_ID,
          location: location ?? baseLocation
        },
        embedded: null,
        source: null
      };

      mutablePlan.nodes ??= {};
      mutablePlan.connections ??= [];
      mutablePlan.nodes[ioNodeId] = ioNode;
      createdNodeId = ioNodeId;

      const connection: PipelineConnection =
        direction === 'input'
          ? { from: { node: ioNodeId, port: normalizedPort }, to: { node: nodeId, port: normalizedPort } }
          : { from: { node: nodeId, port: normalizedPort }, to: { node: ioNodeId, port: normalizedPort } };

      const hasConnection = (mutablePlan.connections ?? []).some(
        (existing) =>
          existing.from.node === connection.from.node &&
          existing.from.port === connection.from.port &&
          existing.to.node === connection.to.node &&
          existing.to.port === connection.to.port
      );
      if (!hasConnection) {
        mutablePlan.connections.push(connection);
      }
    });

    if (createdNodeId) {
      graphSelection.set({ nodeId: createdNodeId, nodes: [createdNodeId], edge: null });
      toaster.success({
        title: 'IO node created',
        description: `Added pipeline ${direction} "${normalizedPort}" using ${typeLabel}.`
      });
      closeGraphContextMenu();
    }

    return createdNodeId;
  }

  function editBoundaryNode(payload: {
    nodeId: string | null;
    direction: 'input' | 'output';
    name: string;
    oldName?: string;
    dataTypeKey: string;
  }): string | null {
    const pipeline = get(selectedPipeline);
    const plan = get(editingPlan);
    if (!pipeline || !plan) return null;
    const nodeId = payload.nodeId?.trim();
    if (!nodeId) return null;
    const trimmedName = payload.name.trim();
    const trimmedType = payload.dataTypeKey.trim();
    if (!trimmedName) {
      toaster.error({ title: 'Name required', description: 'Enter a unique name for the pipeline port.' });
      return null;
    }

    const node = plan.nodes?.[nodeId];
    if (!node) {
      toaster.error({ title: 'Pipeline IO not found', description: 'Selected pipeline IO node could not be located.' });
      return null;
    }
    const nodeDirection = resolveHostIoDirection(node);
    if (!nodeDirection || nodeDirection !== payload.direction) {
      toaster.error({ title: 'Unsupported node', description: 'Only pipeline input/output nodes can be edited here.' });
      return null;
    }

    const portRecord = payload.direction === 'input' ? node.outputs ?? {} : node.inputs ?? {};
    const normalizedOld = normalizePipelinePortName(payload.oldName ?? '');
    const recordKeys = Object.keys(portRecord);
    const existingKey =
      (normalizedOld && recordKeys.find((key) => normalizePipelinePortName(key) === normalizedOld)) ??
      recordKeys.find((key) => normalizePipelinePortName(key) === normalizePipelinePortName(trimmedName)) ??
      (recordKeys.length === 1 ? recordKeys[0] : null);
    if (!existingKey) {
      toaster.error({ title: 'Port not found', description: 'Select a valid pipeline port to edit.' });
      return null;
    }
    const currentPort = normalizePipelinePortName(existingKey);
    const nextPort = normalizePipelinePortName(trimmedName);
    const resolvedTypeKey =
      trimmedType ||
      resolveDataTypeKey(
        payload.direction === 'input'
          ? (node.outputs ?? {})[existingKey] ?? plan.pipelineInputs?.[currentPort]
          : (node.inputs ?? {})[existingKey] ?? plan.pipelineOutputs?.[currentPort]
      ) ||
      'generic';
    const nextType = cloneDataType(buildDataTypeFromKey(resolvedTypeKey));

    if (nextPort !== currentPort) {
      const boundaryRecord = payload.direction === 'input' ? plan.pipelineInputs ?? {} : plan.pipelineOutputs ?? {};
      if (boundaryRecord[nextPort]) {
        toaster.error({
          title: 'Port already exists',
          description: `Pipeline ${payload.direction} "${nextPort}" is already defined.`
        });
        return null;
      }
    }

    let updatedNodeId: string | null = null;
    updateCurrentPlan((mutablePlan) => {
      const targetNode = mutablePlan.nodes?.[nodeId];
      if (!targetNode) return;
      const record = payload.direction === 'input' ? { ...(targetNode.outputs ?? {}) } : { ...(targetNode.inputs ?? {}) };
      const nextRecord: Record<string, PipelineDataType> = {};
      Object.entries(record).forEach(([key, value]) => {
        const normalized = normalizePipelinePortName(key);
        if (normalized === currentPort) {
          nextRecord[nextPort] = nextType;
        } else {
          nextRecord[key] = value;
        }
      });
      if (payload.direction === 'input') {
        targetNode.outputs = nextRecord;
      } else {
        targetNode.inputs = nextRecord;
      }
      ensureHostIoNodeShape(targetNode, payload.direction);

      const updatedConnections = (mutablePlan.connections ?? []).map((connection) => {
        const fromMatch =
          connection.from.node === nodeId && normalizePipelinePortName(connection.from.port) === currentPort;
        const toMatch =
          connection.to.node === nodeId && normalizePipelinePortName(connection.to.port) === currentPort;
        if (!fromMatch && !toMatch) return connection;
        return {
          ...connection,
          from: fromMatch ? { ...connection.from, port: nextPort } : connection.from,
          to: toMatch ? { ...connection.to, port: nextPort } : connection.to
        };
      });
      mutablePlan.connections = updatedConnections;

      if (payload.direction === 'input') {
        const inputs = { ...(mutablePlan.pipelineInputs ?? {}) };
        delete inputs[currentPort];
        inputs[nextPort] = nextType;
        mutablePlan.pipelineInputs = inputs;
        const configs = { ...(mutablePlan.pipelineInputConfigs ?? {}) };
        if (configs[currentPort] !== undefined) {
          configs[nextPort] = configs[currentPort];
          delete configs[currentPort];
        }
        mutablePlan.pipelineInputConfigs = configs;
        const values = { ...(mutablePlan.pipelineInputValues ?? {}) };
        if (values[currentPort] !== undefined) {
          values[nextPort] = values[currentPort];
          delete values[currentPort];
        }
        mutablePlan.pipelineInputValues = values;
      } else {
        const outputs = { ...(mutablePlan.pipelineOutputs ?? {}) };
        delete outputs[currentPort];
        outputs[nextPort] = nextType;
        mutablePlan.pipelineOutputs = outputs;
        const configs = { ...(mutablePlan.pipelineOutputConfigs ?? {}) };
        if (configs[currentPort] !== undefined) {
          configs[nextPort] = configs[currentPort];
          delete configs[currentPort];
        }
        mutablePlan.pipelineOutputConfigs = configs;
      }

      if (mutablePlan.nodeValueOverrides?.[nodeId]) {
        const overrides = { ...(mutablePlan.nodeValueOverrides ?? {}) };
        const nodeOverrides = { ...(overrides[nodeId] ?? {}) };
        const remappedOverrides: Record<string, PipelineNodeValue> = {};
        Object.entries(nodeOverrides).forEach(([port, value]) => {
          const normalizedPort = normalizePipelinePortName(port);
          const targetPort = normalizedPort === currentPort ? nextPort : normalizedPort;
          remappedOverrides[targetPort] = value;
        });
        overrides[nodeId] = remappedOverrides;
        mutablePlan.nodeValueOverrides = Object.keys(overrides[nodeId] ?? {}).length > 0 ? overrides : undefined;
      }

      updatedNodeId = nodeId;
    });
    if (updatedNodeId) {
      graphSelection.set({ nodeId: updatedNodeId, nodes: [updatedNodeId], edge: null });
    }
    return updatedNodeId ?? null;
  }

  function editPipelinePort(
    direction: 'input' | 'output',
    nodeId: string,
    name: string,
    dataTypeKey: string,
    oldName?: string
  ) {
    return editBoundaryNode({ nodeId, direction, name, oldName, dataTypeKey });
  }

  function removePipelinePort(direction: 'input' | 'output', name: string) {
    const pipeline = get(selectedPipeline);
    if (!pipeline) return;
    const portName = normalizePipelinePortName(name);
    if (!portName) return;
    updateCurrentPlan((plan) => {
      removePipelineBoundaryNode(plan, direction, portName);
      if (direction === 'input') {
        const existingValues = { ...(plan.pipelineInputValues ?? {}) };
        if (existingValues[portName] !== undefined) {
          delete existingValues[portName];
          plan.pipelineInputValues = existingValues;
        }
      }
    });
  }

  return {
    addPipelinePort,
    addHostIoPort,
    removeHostIoPort,
    createBoundaryFromPort,
    editBoundaryNode,
    editPipelinePort,
    removePipelinePort
  };
};
