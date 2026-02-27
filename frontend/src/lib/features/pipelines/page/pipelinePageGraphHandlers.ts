import type { PipelineGraphEdgeSelection, PipelineGraphPoint } from '$lib';
import type {
  ChannelPolicy,
  PipelineConnectionStyle,
  PipelineGraphPlan,
  PipelineInputQueueConfig,
  PipelineNodeLayout,
  PipelineNodeValue,
  PipelineOutputSinkConfig
} from '$lib/types/pipeline';

export type GraphContextDetail = {
  type: 'pane' | 'palette' | 'node' | 'port';
  position: PipelineGraphPoint;
  flowPosition: PipelineGraphPoint;
  nodeId?: string | null;
  port?: string | null;
  direction?: 'input' | 'output';
};

export type PipelineGraphHandlersDeps = {
  handlePlanChange: (plan: PipelineGraphPlan) => void;
  handleGraphSelect: (detail: { nodeId: string | null; nodes?: string[]; edge: PipelineGraphEdgeSelection }) => void;
  validateCurrentPipeline: (planOverride?: PipelineGraphPlan | null) => Promise<void> | void;
  getEditingPlan: () => PipelineGraphPlan | null;
  getSelectedPipelineGraph: () => PipelineGraphPlan | null;
  openGraphContext: (detail: GraphContextDetail) => void;
  updateGraphLayout: (pipelineId: string, layout: PipelineNodeLayout) => void;
  enterEmbeddedNode: (nodeId: string) => boolean;
  exitEmbedded: () => void;
  addPipelinePort: (direction: 'input' | 'output', name: string, dataTypeKey: string) => void;
  addHostIoPort: (nodeId: string, name: string, dataTypeKey: string) => void;
  removePipelinePort: (direction: 'input' | 'output', name: string) => void;
  removeHostIoPort: (nodeId: string, name: string) => void;
  editPipelinePort: (direction: 'input' | 'output', nodeId: string, name: string, dataTypeKey: string, oldName?: string) => void;
  setPipelineInputValue: (name: string, value: PipelineNodeValue | null) => void;
  setPipelinePortConfig: (direction: 'input' | 'output', name: string, config: PipelineInputQueueConfig | PipelineOutputSinkConfig) => void;
  setNodeConstantValue: (nodeId: string, port: string, value: PipelineNodeValue | null) => void;
  setGraphConnectionPolicy: (connection: PipelineGraphEdgeSelection, policy: ChannelPolicy) => void;
  setGraphConnectionStyle: (connection: PipelineGraphEdgeSelection, style: PipelineConnectionStyle) => void;
  toaster: { info: (payload: { title: string; description?: string }) => void };
};

export const createPipelineGraphHandlers = (deps: PipelineGraphHandlersDeps) => {
  const handlePanelPlanChange = (event: CustomEvent<{ plan: PipelineGraphPlan }>) => {
    deps.handlePlanChange(event.detail.plan);
  };

  const handlePanelGraphSelect = (event: CustomEvent<{ nodeId: string | null; nodes: string[]; edge: PipelineGraphEdgeSelection }>) => {
    deps.handleGraphSelect(event.detail);
  };

  const handlePipelineValidate = () => {
    const plan = deps.getEditingPlan() ?? deps.getSelectedPipelineGraph() ?? null;
    void deps.validateCurrentPipeline(plan);
  };

  const handlePanelGraphContext = (event: CustomEvent<GraphContextDetail>) => {
    deps.openGraphContext(event.detail);
  };

  const handlePanelGraphLayout = (event: CustomEvent<{ pipelineId: string; layout: PipelineNodeLayout }>) => {
    deps.updateGraphLayout(event.detail.pipelineId, event.detail.layout);
  };

  const handleEnterEmbedded = (event: CustomEvent<{ nodeId: string }>) => {
    const entered = deps.enterEmbeddedNode(event.detail.nodeId);
    if (!entered) {
      deps.toaster.info({ title: 'Cannot open group', description: 'Only embedded pipelines can be opened right now.' });
    }
  };

  const handleExitEmbedded = () => {
    deps.exitEmbedded();
  };

  const handlePipelinePortAdd = (event: CustomEvent<{ direction: 'input' | 'output'; name: string; dataTypeKey: string }>) => {
    deps.addPipelinePort(event.detail.direction, event.detail.name, event.detail.dataTypeKey);
  };

  const handleHostIoPortAdd = (event: CustomEvent<{ nodeId: string; name: string; dataTypeKey: string }>) => {
    deps.addHostIoPort(event.detail.nodeId, event.detail.name, event.detail.dataTypeKey);
  };

  const handlePipelinePortRemove = (event: CustomEvent<{ direction: 'input' | 'output'; name: string }>) => {
    deps.removePipelinePort(event.detail.direction, event.detail.name);
  };

  const handleHostIoPortRemove = (event: CustomEvent<{ nodeId: string; name: string }>) => {
    deps.removeHostIoPort(event.detail.nodeId, event.detail.name);
  };

  const handlePipelinePortEdit = (
    event: CustomEvent<{ direction: 'input' | 'output'; nodeId: string; name: string; oldName?: string; dataTypeKey: string }>
  ) => {
    deps.editPipelinePort(event.detail.direction, event.detail.nodeId, event.detail.name, event.detail.dataTypeKey, event.detail.oldName);
  };

  const handlePipelinePortValue = (event: CustomEvent<{ direction: 'input' | 'output'; name: string; value: PipelineNodeValue | null }>) => {
    if (event.detail.direction === 'input') {
      deps.setPipelineInputValue(event.detail.name, event.detail.value);
    }
  };

  const handlePipelinePortConfig = (
    event: CustomEvent<{
      direction: 'input' | 'output';
      name: string;
      config: PipelineInputQueueConfig | PipelineOutputSinkConfig;
    }>
  ) => {
    deps.setPipelinePortConfig(event.detail.direction, event.detail.name, event.detail.config);
  };

  const handleNodeConstantValue = (event: CustomEvent<{ nodeId: string; port: string; value: PipelineNodeValue | null }>) => {
    deps.setNodeConstantValue(event.detail.nodeId, event.detail.port, event.detail.value);
  };

  const handleEdgePolicy = (event: CustomEvent<{ connection: PipelineGraphEdgeSelection; policy: ChannelPolicy }>) => {
    const { connection, policy } = event.detail;
    deps.setGraphConnectionPolicy(connection, policy);
  };

  const handleEdgeStyle = (event: CustomEvent<{ connection: PipelineGraphEdgeSelection; style: PipelineConnectionStyle }>) => {
    const { connection, style } = event.detail;
    deps.setGraphConnectionStyle(connection, style);
  };

  return {
    handlePanelPlanChange,
    handlePanelGraphSelect,
    handlePipelineValidate,
    handlePanelGraphContext,
    handlePanelGraphLayout,
    handleEnterEmbedded,
    handleExitEmbedded,
    handlePipelinePortAdd,
    handleHostIoPortAdd,
    handlePipelinePortRemove,
    handleHostIoPortRemove,
    handlePipelinePortEdit,
    handlePipelinePortValue,
    handlePipelinePortConfig,
    handleNodeConstantValue,
    handleEdgePolicy,
    handleEdgeStyle
  };
};
