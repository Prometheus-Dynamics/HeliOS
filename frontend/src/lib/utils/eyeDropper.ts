type EyeDropperResult = { sRGBHex: string };
type EyeDropperInstance = { open: () => Promise<EyeDropperResult> };
type EyeDropperCtor = new () => EyeDropperInstance;

type EyeDropperWindow = { EyeDropper?: EyeDropperCtor };

const resolveEyeDropper = (): EyeDropperCtor | null => {
  if (typeof window !== 'undefined') {
    const ctor = (window as EyeDropperWindow).EyeDropper;
    if (ctor) return ctor;
  }
  if (typeof globalThis !== 'undefined') {
    const ctor = (globalThis as EyeDropperWindow).EyeDropper;
    if (ctor) return ctor;
  }
  return null;
};

export const supportsEyeDropper = (): boolean => {
  return typeof resolveEyeDropper() === 'function';
};

export const pickEyeDropperColor = async (): Promise<string | null> => {
  const EyeDropper = resolveEyeDropper();
  if (!EyeDropper) return null;
  try {
    const eyeDropper = new EyeDropper();
    const result = await eyeDropper.open();
    if (!result || typeof result.sRGBHex !== 'string') return null;
    return result.sRGBHex;
  } catch {
    return null;
  }
};
