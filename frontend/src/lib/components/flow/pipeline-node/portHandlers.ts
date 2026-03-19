import { toaster } from '$lib';
import { isDataTypeSettable, resolveDataTypeKey } from '$lib/features/pipelines/valueFormatting';
import type { PipelineGraphNode } from '$lib/types/pipeline';
import { typeKey } from '../pipeline-graph/utils';
import { findPixelColorInput } from './pixelInputs';
import type { PipelineNodeData, PortInteractionHandlers, PortRenderInfo } from './types';

export function createPortHandlers(context: {
  data: PipelineNodeData | undefined;
  node: PipelineGraphNode | undefined;
  nodeBaseId: string;
}): PortInteractionHandlers {
  const resolveNodeId = (): string | null => {
    return context.node?.id ?? context.node?.info?.id ?? context.nodeBaseId ?? null;
  };

  function openPortEditor(direction: 'input' | 'output', port: PortRenderInfo, event: MouseEvent) {
    context.data?.onPortDoubleClick?.({ direction, port: port.name, event });
  }

  function toggleBooleanConstant(port: PortRenderInfo) {
    const nodeId = resolveNodeId();
    if (!nodeId) return;
    context.data?.onPortBooleanConstantToggle?.({ nodeId, port: port.name, enabled: !port.hasConstant });
  }

  function toggleEnumConstant(port: PortRenderInfo): boolean {
    const nodeId = resolveNodeId();
    if (!nodeId) return false;
    if (port.hasConstant) {
      context.data?.onPortEnumChange?.({ nodeId, port: port.name, value: null });
      return true;
    }
    const nextValue = port.variants[0]?.raw ?? null;
    if (!nextValue) {
      return false;
    }
    context.data?.onPortEnumChange?.({ nodeId, port: port.name, value: nextValue });
    return true;
  }

  function toggleNumericConstant(port: PortRenderInfo) {
    const nodeId = resolveNodeId();
    if (!nodeId) return;
    context.data?.onPortNumericConstantToggle?.({ nodeId, port: port.name, enabled: !port.hasConstant });
  }

  function togglePixelConstant(port: PortRenderInfo) {
    const nodeId = resolveNodeId();
    if (!nodeId) return;
    context.data?.onPortPixelConstantToggle?.({ nodeId, port: port.name, enabled: !port.hasConstant });
  }

  function handlePortDoubleClick(direction: 'input' | 'output', port: PortRenderInfo, event: MouseEvent) {
    event.stopPropagation();
    if (direction !== 'input') return;
    if (port.hasConstant) {
      const nodeId = resolveNodeId();
      if (!nodeId) return;
      if (port.isEnum) {
        context.data?.onPortEnumChange?.({ nodeId, port: port.name, value: null });
        return;
      }
      if (port.isBoolean) {
        context.data?.onPortBooleanConstantToggle?.({ nodeId, port: port.name, enabled: false });
        return;
      }
      if (port.isNumeric) {
        context.data?.onPortNumericConstantToggle?.({ nodeId, port: port.name, enabled: false });
        return;
      }
      if (port.isPixel) {
        context.data?.onPortPixelConstantToggle?.({ nodeId, port: port.name, enabled: false });
        return;
      }
      context.data?.onPortClear?.({ nodeId, port: port.name });
      return;
    }
    const isSettable = port.settable && isDataTypeSettable(port.type);
    if (!isSettable) {
      const resolvedKey =
        resolveDataTypeKey(port.type) ?? typeKey(port.type) ?? (typeof port.type === 'string' ? port.type : null);
      const typeLabel = resolvedKey ?? port.label ?? 'Unknown type';
      toaster.warning({
        title: 'Port is not settable',
        description: `${port.name} (${typeLabel}) cannot accept manual values.`
      });
      return;
    }
    if (port.isBoolean) {
      toggleBooleanConstant(port);
      return;
    }
    if (port.isEnum) {
      const toggled = toggleEnumConstant(port);
      if (!toggled) {
        openPortEditor(direction, port, event);
      }
      return;
    }
    if (port.isNumeric) {
      toggleNumericConstant(port);
      return;
    }
    if (port.isPixel) {
      togglePixelConstant(port);
      return;
    }
    openPortEditor(direction, port, event);
  }

  function handlePortContextMenu(direction: 'input' | 'output', port: PortRenderInfo, event: MouseEvent) {
    event.preventDefault();
    event.stopPropagation();
    const nodeId = resolveNodeId();
    if (!nodeId) return;
    context.data?.onPortContextMenu?.({ direction, port: port.name, event });
  }

  function handleEnumSelect(port: string, event: Event) {
    event.stopPropagation();
    const select = event.currentTarget as HTMLSelectElement | null;
    if (!select) return;
    const nodeId = resolveNodeId();
    if (!nodeId) return;
    context.data?.onPortEnumChange?.({ nodeId, port, value: select.value });
  }

  function handleBooleanToggle(port: string, event: Event) {
    event.stopPropagation();
    const input = event.currentTarget as HTMLInputElement | null;
    if (!input) return;
    const nodeId = resolveNodeId();
    if (!nodeId) return;
    context.data?.onPortBooleanToggle?.({ nodeId, port, value: input.checked });
  }

  function handlePixelPreviewClick(port: PortRenderInfo, event: MouseEvent) {
    event.stopPropagation();
    const input = findPixelColorInput(port.handleId);
    if (!input) return;
    const picker = (input as HTMLInputElement & { showPicker?: () => void }).showPicker;
    if (typeof picker === 'function') {
      picker.call(input);
    } else {
      input.click();
    }
  }

  function handlePixelColorInput(port: PortRenderInfo, event: Event) {
    event.stopPropagation();
    const input = event.currentTarget as HTMLInputElement | null;
    if (!input) return;
    const nodeId = resolveNodeId();
    if (!nodeId) return;
    context.data?.onPortPixelColorChange?.({ nodeId, port: port.name, hex: input.value });
  }

  function handleNumericInput(port: PortRenderInfo, event: Event) {
    event.stopPropagation();
    const target = event.currentTarget as HTMLInputElement | null;
    if (!target) return;
    const nodeId = resolveNodeId();
    if (!nodeId) return;
    if (target.value === '') {
      return;
    }
    context.data?.onPortNumericValueChange?.({ nodeId, port: port.name, value: target.value ?? '' });
  }

  function handleNumericBlur(port: PortRenderInfo, event: FocusEvent) {
    const target = event.currentTarget as HTMLInputElement | null;
    if (!target) return;
    if (target.value.trim() === '') {
      const nodeId = resolveNodeId();
      if (!nodeId) return;
      context.data?.onPortNumericConstantToggle?.({ nodeId, port: port.name, enabled: false });
    }
  }

  function handleNumericBeforeInput(port: PortRenderInfo, event: InputEvent) {
    if (event.inputType !== 'insertText') return;
    const data = event.data;
    if (!data) return;
    const allowPattern = port.numericStep === '1' ? /^[0-9+-]$/ : /^[0-9.+-]$/;
    if (!allowPattern.test(data)) {
      event.preventDefault();
    }
  }

  return {
    handlePortDoubleClick,
    handlePortContextMenu,
    handleEnumSelect,
    handleBooleanToggle,
    handleNumericInput,
    handleNumericBlur,
    handleNumericBeforeInput,
    handlePixelPreviewClick,
    handlePixelColorInput
  };
}
