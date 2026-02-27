const pixelColorInputs = new Map<string, HTMLInputElement>();

const registerPixelColorInput = (handleId: string, element: HTMLInputElement | null) => {
  if (element) {
    pixelColorInputs.set(handleId, element);
  } else {
    pixelColorInputs.delete(handleId);
  }
};

export const pixelColorInputAction = (node: HTMLInputElement, handleId: string) => {
  registerPixelColorInput(handleId, node);
  return {
    destroy() {
      registerPixelColorInput(handleId, null);
    }
  };
};

export const findPixelColorInput = (handleId: string): HTMLInputElement | undefined =>
  pixelColorInputs.get(handleId);
