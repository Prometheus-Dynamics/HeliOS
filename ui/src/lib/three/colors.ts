// three.js needs real colours; identity colours are CSS variables.
export function cssVarColor(cssValue: string, fallback = "#9aa0ae"): string {
  const m = /var\((--[\w-]+)\)/.exec(cssValue);
  if (!m || typeof document === "undefined") return cssValue.startsWith("var(") ? fallback : cssValue;
  return getComputedStyle(document.documentElement).getPropertyValue(m[1]).trim() || fallback;
}
