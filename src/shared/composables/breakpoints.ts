/** Single source of truth for layout breakpoints (px). Keep in sync with tokens.css docs. */
export const BREAKPOINTS = {
  xl: 1200,
  lg: 1080,
  md: 900,
  sm: 720,
  xs: 640,
  /** Compact nav threshold used by AppShell (between md and lg). */
  compact: 1023,
} as const;

export type BreakpointName = keyof typeof BREAKPOINTS;

export function mediaMaxWidth(px: number): string {
  return `(max-width: ${px}px)`;
}

export function mediaMinResolution(dppx: number): string {
  return `(min-resolution: ${dppx}dppx)`;
}
