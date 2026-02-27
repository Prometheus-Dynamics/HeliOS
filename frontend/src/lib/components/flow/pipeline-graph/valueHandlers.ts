import { clampEditorPosition } from './utils';
import { toaster } from '$lib';
import {
  applyPortEditorDraft,
  buildNodePortEditorState,
  clearPortEditorValue as clearPortEditorValueFromPlan
} from './portEditor';
import {
  buildNodeValueFromInput,
  isDataTypeSettable,
  resolveDataTypeKey
} from '$lib/features/pipelines/valueFormatting';
import {
  DEFAULT_NUMERIC_VALUE,
  DEFAULT_PIXEL_COLOR,
  hexToRgb,
  isBooleanTypeKey,
  isNumericTypeKey,
  isPixelTypeKey,
  parsePixelDraft,
  parsePixelValue
} from './editorUtils';
import { ensurePlanPortMetadata } from './utils';
import type { PipelineDataType, PipelineGraphNode, PipelineGraphPlan, PipelineNodeValue } from '$lib/types/pipeline';
import type {
  BooleanConstantHandler,
  BooleanToggleHandler,
  EnumSelectionHandler,
  NumericConstantToggleHandler,
  NumericValueChangeHandler,
  PixelColorChangeHandler,
  PixelConstantToggleHandler,
  PortEditorState
} from './types';

type Getter<T> = () => T;
type Setter<T> = (value: T) => void;

export type ValueController = ReturnType<typeof createValueController>;

const resolveDataType = (dataType: PipelineDataType | undefined, fallbackKey?: string): PipelineDataType | string =>
  dataType ?? fallbackKey ?? 'Generic';

const isNodePortLocked = (node: PipelineGraphNode | null | undefined): boolean =>
  Boolean(node && node.backendId?.toLowerCase() === 'pipeline:child' && node.external);

export const createValueController = (deps: {
  getPlan: Getter<PipelineGraphPlan>;
  setPlan: Setter<PipelineGraphPlan>;
  commitPlan: () => void;
  interactive: () => boolean;
  getPortEditor: Getter<PortEditorState | null>;
  setPortEditor: Setter<PortEditorState | null>;
  getDraft: Getter<string>;
  setDraft: Setter<string>;
  getError: Getter<string | null>;
  setError: Setter<string | null>;
  getIsPixelPortEditor: Getter<boolean>;
  getPixelEditorState: Getter<ReturnType<typeof parsePixelDraft> | ReturnType<typeof parsePixelValue> | null>;
  closePortEditor: () => void;
  undoPlan: () => boolean;
  redoPlan: () => boolean;
}) => {
  const updateNodeValue = (
    nodeId: string,
    port: string,
    value: PipelineNodeValue | null
  ): boolean => {
    const plan = deps.getPlan();
    const node = plan.nodes?.[nodeId];
    if (!node || isNodePortLocked(node)) return false;
    const existing = node.info.values ?? {};
    const nextValues = { ...existing };
    if (value === null) {
      delete nextValues[port];
    } else {
      nextValues[port] = value;
    }
    const sanitizedValues = Object.keys(nextValues).length > 0 ? nextValues : undefined;
    const nextPlan = ensurePlanPortMetadata({
      ...plan,
      nodes: {
        ...plan.nodes,
        [nodeId]: {
          ...node,
          info: { ...node.info, values: sanitizedValues }
        }
      }
    });
    deps.setPlan(nextPlan);
    deps.commitPlan();
    return true;
  };

  const clearNodePortValue = (nodeId: string, port: string): void => {
    updateNodeValue(nodeId, port, null);
  };

  const openPortEditor = (
    nodeId: string,
    port: string,
    event: MouseEvent,
    direction: 'input' | 'output'
  ) => {
    if (!deps.interactive() || direction !== 'input') return;
    const anchor = clampEditorPosition({ x: event.clientX + 18, y: event.clientY - 12 });
    const plan = deps.getPlan();
    const node = plan.nodes?.[nodeId];
    if (isNodePortLocked(node)) return;
    const result = buildNodePortEditorState(plan, nodeId, port, anchor);
    if (!result) return;
    if (!result.state.settable) {
      const typeLabel = result.state.dataTypeKey ?? 'Unknown type';
      toaster.warning({
        title: 'Port is not settable',
        description: `${port} (${typeLabel}) cannot accept manual values.`
      });
      return;
    }
    deps.setPortEditor(result.state);
    deps.setDraft(result.draft);
    deps.setError(result.error);
  };

  const handleEnumChange: EnumSelectionHandler = ({ nodeId, port, value }) => {
    if (!deps.interactive()) return;
    const planNode = deps.getPlan().nodes?.[nodeId];
    if (!planNode || isNodePortLocked(planNode)) return;
    const dataType = planNode.inputs?.[port];
    if (!isDataTypeSettable(dataType)) return;
    if (value === null) {
      updateNodeValue(nodeId, port, null);
      return;
    }
    const dataTypeResolved = resolveDataType(dataType, resolveDataTypeKey(dataType) ?? undefined);
    updateNodeValue(nodeId, port, { dataType: dataTypeResolved, value });
  };

  const handleBooleanToggle: BooleanToggleHandler = ({ nodeId, port, value }) => {
    if (!deps.interactive()) return;
    const planNode = deps.getPlan().nodes?.[nodeId];
    if (!planNode || isNodePortLocked(planNode)) return;
    const dataType = planNode.inputs?.[port];
    if (!isDataTypeSettable(dataType)) return;
    const dataTypeKey = resolveDataTypeKey(dataType);
    if (!dataTypeKey || !isBooleanTypeKey(dataTypeKey)) return;
    updateNodeValue(nodeId, port, { dataType: dataTypeKey, value });
  };

  const handleBooleanConstantToggle: BooleanConstantHandler = ({ nodeId, port, enabled }) => {
    if (!deps.interactive()) return;
    const planNode = deps.getPlan().nodes?.[nodeId];
    if (!planNode || isNodePortLocked(planNode)) return;
    const dataType = planNode.inputs?.[port];
    if (!isDataTypeSettable(dataType)) return;
    const dataTypeKey = resolveDataTypeKey(dataType);
    if (!dataTypeKey || !isBooleanTypeKey(dataTypeKey)) return;
    if (!enabled) {
      updateNodeValue(nodeId, port, null);
      return;
    }
    updateNodeValue(nodeId, port, { dataType: dataTypeKey, value: false });
  };

  const handlePixelConstantToggle: PixelConstantToggleHandler = ({ nodeId, port, enabled }) => {
    if (!deps.interactive()) return;
    const planNode = deps.getPlan().nodes?.[nodeId];
    if (!planNode || isNodePortLocked(planNode)) return;
    const dataType = planNode.inputs?.[port];
    if (!isDataTypeSettable(dataType)) return;
    const dataTypeKey = resolveDataTypeKey(dataType);
    if (!dataTypeKey || !isPixelTypeKey(dataTypeKey)) return;
    if (!enabled) {
      updateNodeValue(nodeId, port, null);
      return;
    }
    updateNodeValue(nodeId, port, { dataType: dataTypeKey, value: DEFAULT_PIXEL_COLOR });
  };

  const handlePixelColorChange: PixelColorChangeHandler = ({ nodeId, port, hex }) => {
    if (!deps.interactive()) return;
    const planNode = deps.getPlan().nodes?.[nodeId];
    if (!planNode || isNodePortLocked(planNode)) return;
    const dataType = planNode.inputs?.[port];
    if (!isDataTypeSettable(dataType)) return;
    const dataTypeKey = resolveDataTypeKey(dataType);
    if (!dataTypeKey || !isPixelTypeKey(dataTypeKey)) return;
    const rgb = hexToRgb(hex);
    if (!rgb) return;
    updateNodeValue(nodeId, port, {
      dataType: dataTypeKey,
      value: { ...DEFAULT_PIXEL_COLOR, ...rgb }
    });
  };

  const handlePixelColorInput = (event: Event) => {
    const portEditor = deps.getPortEditor();
    if (!portEditor || !deps.getIsPixelPortEditor() || !portEditor.settable) return;
    const input = event.currentTarget as HTMLInputElement | null;
    if (!input) return;
    const rgb = hexToRgb(input.value);
    if (!rgb) return;
    const base = deps.getPixelEditorState() ?? DEFAULT_PIXEL_COLOR;
    const next = { ...base, ...rgb };
    deps.setDraft(JSON.stringify(next));
    deps.setError(null);
    applyPortEditor();
  };

  const handleNumericConstantToggle: NumericConstantToggleHandler = ({ nodeId, port, enabled }) => {
    if (!deps.interactive()) return;
    const planNode = deps.getPlan().nodes?.[nodeId];
    if (!planNode || isNodePortLocked(planNode)) return;
    const dataType = planNode.inputs?.[port];
    if (!isDataTypeSettable(dataType)) return;
    const dataTypeKey = resolveDataTypeKey(dataType);
    if (!dataTypeKey || !isNumericTypeKey(dataTypeKey)) return;
    if (!enabled) {
      updateNodeValue(nodeId, port, null);
      return;
    }
    const dataTypeResolved = resolveDataType(dataType, resolveDataTypeKey(dataType) ?? undefined);
    updateNodeValue(nodeId, port, { dataType: dataTypeResolved, value: DEFAULT_NUMERIC_VALUE });
  };

  const handleNumericValueChange: NumericValueChangeHandler = ({ nodeId, port, value }) => {
    if (!deps.interactive()) return;
    const planNode = deps.getPlan().nodes?.[nodeId];
    if (!planNode || isNodePortLocked(planNode)) return;
    const dataType = planNode.inputs?.[port];
    if (!isDataTypeSettable(dataType)) return;
    const dataTypeKey = resolveDataTypeKey(dataType);
    if (!dataTypeKey || !isNumericTypeKey(dataTypeKey)) return;
    const parsed = buildNodeValueFromInput(value, dataTypeKey);
    if (!parsed.success) {
      deps.setError(parsed.error);
      return;
    }
    const numericValue = typeof parsed.value.value === 'number' ? parsed.value.value : null;
    if (numericValue == null) return;
    updateNodeValue(nodeId, port, parsed.value);
  };

  const applyPortEditor = () => {
    const portEditor = deps.getPortEditor();
    if (!portEditor) return;
    const result = applyPortEditorDraft(deps.getPlan(), portEditor, deps.getDraft());
    if (!result.success) {
      deps.setError(result.error);
      return;
    }
    deps.setPlan(ensurePlanPortMetadata(result.plan));
    deps.commitPlan();
    deps.setError(null);
    deps.setPortEditor(null);
    deps.setDraft('');
  };

  const clearPortEditor = () => {
    const portEditor = deps.getPortEditor();
    if (!portEditor) return;
    const result = clearPortEditorValueFromPlan(deps.getPlan(), portEditor);
    if (!result.success) {
      if ('error' in result) {
        deps.setError(result.error ?? 'Unable to clear value.');
      }
      return;
    }
    deps.setPlan(ensurePlanPortMetadata(result.plan));
    deps.commitPlan();
    deps.setPortEditor(null);
    deps.setDraft('');
    deps.setError(null);
  };

  const handleWindowKeydown = (event: KeyboardEvent) => {
    const portEditor = deps.getPortEditor();
    if (portEditor && event.key === 'Escape') {
      event.preventDefault();
      deps.setPortEditor(null);
      deps.setDraft('');
      deps.setError(null);
      return;
    }
    if (!deps.interactive()) return;
    if (!(event.metaKey || event.ctrlKey)) return;
    if (portEditor) return;
    const key = typeof event.key === 'string' ? event.key.toLowerCase() : '';
    const code = typeof event.code === 'string' ? event.code.toLowerCase() : '';
    const isUndoKey = key === 'z' || code === 'keyz';
    if (!isUndoKey) return;
    if (event.shiftKey) {
      if (deps.redoPlan()) {
        event.preventDefault();
      }
      return;
    }
    if (deps.undoPlan()) {
      event.preventDefault();
    }
  };

  return {
    openPortEditor,
    handleEnumChange,
    handleBooleanToggle,
    handleBooleanConstantToggle,
    handlePixelConstantToggle,
    handlePixelColorChange,
    handlePixelColorInput,
    handleNumericConstantToggle,
    handleNumericValueChange,
    clearNodePortValue,
    applyPortEditor,
    clearPortEditor,
    handleWindowKeydown
  };
};
