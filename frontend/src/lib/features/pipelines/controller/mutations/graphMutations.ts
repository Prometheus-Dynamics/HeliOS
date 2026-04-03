import { get } from 'svelte/store';
import type { PipelineGraphEdgeSelection } from '$lib';
import type { ChannelPolicy, PipelineConnectionStyle } from '$lib/types/pipeline';
import { normalizePipelinePortName, refreshPipelineIoCaches } from '../../boundary';
import { normalizeConnectionStyle } from '$lib/components/flow/pipeline-graph/edgeStyle';
import { groupSelectionInPlace, ungroupSelectionInPlace } from '../../grouping';
import type { PipelineMutationsDeps } from './types';

export const createGraphMutations = (deps: PipelineMutationsDeps) => {
  const { selectedPipeline, editingPlan, graphSelection, updateCurrentPlan, closeGraphContextMenu, resolveHostIoDirection, generateNodeId } = deps;

  function groupSelection() {
    const pipeline = get(selectedPipeline);
    const plan = get(editingPlan);
    const selectionState = get(graphSelection);
    const selectedIds = selectionState.nodes?.length
      ? selectionState.nodes
      : selectionState.nodeId
        ? [selectionState.nodeId]
        : [];
    if (!pipeline || !plan || selectedIds.length === 0) return;
    const existingIds = selectedIds.filter((id) => plan.nodes?.[id]);
    if (existingIds.length === 0) return;
    const selectedSet = new Set(existingIds);
    const primaryId =
      selectionState.nodeId && selectedSet.has(selectionState.nodeId) ? selectionState.nodeId : existingIds[0];
    const target = plan.nodes?.[primaryId];
    if (!target) return;

    let createdGroupId: string | null = null;
    updateCurrentPlan((currentPlan) => {
      const id = groupSelectionInPlace({
        plan: currentPlan,
        selectedNodeIds: existingIds,
        primaryNodeId: primaryId,
        generateNodeId
      });
      createdGroupId = id;
    });

    if (createdGroupId) {
      graphSelection.set({ nodeId: createdGroupId, nodes: [createdGroupId], edge: null });
    } else {
      graphSelection.set({ nodeId: null, nodes: [], edge: null });
    }
    closeGraphContextMenu();
  }

  function ungroupSelection() {
    const selection = get(graphSelection).nodeId;
    const plan = get(editingPlan);
    if (!selection || !plan) return;
    const node = plan.nodes?.[selection];
    if (!node || node.backendId.toLowerCase() !== 'pipeline:child' || !node.embedded) return;

    updateCurrentPlan((currentPlan) => {
      ungroupSelectionInPlace({ plan: currentPlan, groupNodeId: selection, generateNodeId });
    });

    graphSelection.set({ nodeId: null, nodes: [], edge: null });
    closeGraphContextMenu();
  }

  function removeGraphNode(nodeId: string | null) {
    const pipeline = get(selectedPipeline);
    if (!pipeline || !nodeId) return;
    updateCurrentPlan((plan) => {
      const node = plan.nodes?.[nodeId];
      if (!node) return;
      const direction = resolveHostIoDirection(node);
      const portNames =
        direction === 'input'
          ? Object.keys(node.outputs ?? {})
          : direction === 'output'
            ? Object.keys(node.inputs ?? {})
            : [];
      delete plan.nodes[nodeId];
      plan.connections = plan.connections.filter(
        (connection) => connection.from.node !== nodeId && connection.to.node !== nodeId
      );
      if (direction && portNames.length > 0) {
        portNames.forEach((port) => {
          const key = normalizePipelinePortName(port);
          if (!key) return;
          if (direction === 'input') {
            if (plan.pipelineInputs) {
              delete plan.pipelineInputs[key];
            }
            if (plan.pipelineInputConfigs) {
              delete plan.pipelineInputConfigs[key];
            }
            if (plan.pipelineInputValues) {
              delete plan.pipelineInputValues[key];
            }
          } else if (direction === 'output') {
            if (plan.pipelineOutputs) {
              delete plan.pipelineOutputs[key];
            }
            if (plan.pipelineOutputConfigs) {
              delete plan.pipelineOutputConfigs[key];
            }
          }
        });
      }
      refreshPipelineIoCaches(plan);
    });
    graphSelection.set({ nodeId: null, nodes: [], edge: null });
    closeGraphContextMenu();
  }

  function removeGraphConnection(selection: PipelineGraphEdgeSelection | null) {
    const pipeline = get(selectedPipeline);
    if (!pipeline || !selection) return;
    updateCurrentPlan((plan) => {
      plan.connections = plan.connections.filter(
        (connection) =>
          !(
            connection.from.node === selection.from.node &&
            connection.from.port === selection.from.port &&
            connection.to.node === selection.to.node &&
            connection.to.port === selection.to.port
          )
      );
    });
    graphSelection.set({ nodeId: null, nodes: [], edge: null });
    closeGraphContextMenu();
  }

  function setGraphConnectionPolicy(connection: PipelineGraphEdgeSelection | null, policy: ChannelPolicy) {
    const pipeline = get(selectedPipeline);
    if (!pipeline || !connection) return;
    if (pipeline.graph?.format === 'daedalus') return;
    updateCurrentPlan((plan) => {
      const match = plan.connections.find(
        (candidate) =>
          candidate.from.node === connection.from.node &&
          candidate.from.port === connection.from.port &&
          candidate.to.node === connection.to.node &&
          candidate.to.port === connection.to.port
      );
      if (!match) {
        return;
      }
      match.policy = policy;
    });
  }

  function setGraphConnectionStyle(connection: PipelineGraphEdgeSelection | null, style: PipelineConnectionStyle) {
    const pipeline = get(selectedPipeline);
    if (!pipeline || !connection) return;
    updateCurrentPlan((plan) => {
      const match = plan.connections.find(
        (candidate) =>
          candidate.from.node === connection.from.node &&
          candidate.from.port === connection.from.port &&
          candidate.to.node === connection.to.node &&
          candidate.to.port === connection.to.port
      );
      if (!match) {
        return;
      }
      match.style = normalizeConnectionStyle({
        ...(match.style ?? {}),
        ...(style ?? {})
      });
    });
  }

  return {
    groupSelection,
    ungroupSelection,
    removeGraphNode,
    removeGraphConnection,
    setGraphConnectionPolicy,
    setGraphConnectionStyle
  };
};
