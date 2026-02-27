export type XtermDeps = {
  Terminal: typeof import('xterm').Terminal;
  FitAddon: typeof import('xterm-addon-fit').FitAddon;
};

type TerminalOptions = import('xterm').ITerminalOptions;

const DEFAULT_FONT_FAMILY = 'JetBrains Mono, ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace';

const DEFAULT_THEME = {
  background: '#030712',
  foreground: '#f8fafc'
};

let depsPromise: Promise<XtermDeps> | null = null;

export async function loadXtermDeps(): Promise<XtermDeps> {
  if (!depsPromise) {
    depsPromise = Promise.all([import('xterm'), import('xterm-addon-fit')]).then(([xterm, fit]) => ({
      Terminal: xterm.Terminal,
      FitAddon: fit.FitAddon
    }));
  }
  return depsPromise;
}

export function createTerminal(
  deps: XtermDeps,
  options: Partial<TerminalOptions> = {}
): { terminal: import('xterm').Terminal; fitAddon: import('xterm-addon-fit').FitAddon } {
  const terminal = new deps.Terminal({
    cursorBlink: false,
    fontSize: 13,
    fontFamily: DEFAULT_FONT_FAMILY,
    theme: DEFAULT_THEME,
    allowTransparency: true,
    scrollback: 5000,
    convertEol: true,
    ...options
  });
  const fitAddon = new deps.FitAddon();
  terminal.loadAddon(fitAddon);
  return { terminal, fitAddon };
}
