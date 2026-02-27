import type { XYPosition } from '@xyflow/system';

type WindowKeydownOptions = {
  interactive: () => boolean;
  isEditableTarget: (target: EventTarget | null) => boolean;
  getLastPointer: () => XYPosition | null;
  getLastFlowPointer: () => XYPosition | null;
  toFlowPosition: (position: XYPosition) => XYPosition;
  dispatchContext: (payload: {
    type: 'palette';
    position: XYPosition;
    flowPosition: XYPosition;
    nodeId: null;
  }) => void;
  rawHandleWindowKeydown: (event: KeyboardEvent) => void;
};

export const createWindowKeydownHandler = ({
  interactive,
  isEditableTarget,
  getLastPointer,
  getLastFlowPointer,
  toFlowPosition,
  dispatchContext,
  rawHandleWindowKeydown
}: WindowKeydownOptions) =>
  (event: KeyboardEvent) => {
    if (isEditableTarget(event.target)) return;
    if (event.code === 'Space' && !event.repeat && interactive()) {
      event.preventDefault();
      const fallback =
        typeof window !== 'undefined'
          ? { x: window.innerWidth / 2, y: window.innerHeight / 2 }
          : { x: 0, y: 0 };
      const client = getLastPointer() ?? fallback;
      const flowPosition = getLastFlowPointer() ?? toFlowPosition(client);
      dispatchContext({ type: 'palette', position: client, flowPosition, nodeId: null });
      return;
    }
    rawHandleWindowKeydown(event);
  };
