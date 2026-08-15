export const mochaPalette = {
  crust: "#11111b", mantle: "#181825", base: "#1e1e2e",
  surface0: "#313244", surface1: "#45475a", surface2: "#585b70",
  overlay0: "#6c7086", subtext0: "#a6adc8", text: "#cdd6f4",
  mauve: "#cba6f7", blue: "#89b4fa", green: "#a6e3a1",
  yellow: "#f9e2af", red: "#f38ba8", peach: "#fab387",
} as const;

/** Official Rose Pine Moon. Conversation surface only; other pages stay Mocha. */
export const rosePineMoonPalette = {
  base: "#232136",
  surface: "#2a273f",
  overlay: "#393552",
  muted: "#6e6a86",
  subtle: "#908caa",
  text: "#e0def4",
  iris: "#c4a7e7",
  love: "#eb6f92",
  foam: "#9ccfd8",
  gold: "#f6c177",
  pine: "#3e8fb0",
  rose: "#ea9a97",
  highlightMed: "#44415a",
  highlightHigh: "#56526e",
  /**
   * Subtle mixed toward text so secondary copy stays 4.5:1 on overlay cards.
   * Official muted/subtle stay in the palette; conversation remaps to this.
   */
  readableMuted: "#b4b1cb",
} as const;

export type MochaToken = keyof typeof mochaPalette;
export type RosePineMoonToken = keyof typeof rosePineMoonPalette;

export const cssTokenName = (token: MochaToken): string => `--ctp-${token}`;

/** Existing `--ctp-*` slots remapped so conversation CSS keeps one token language. */
export const conversationSurfaceTokens: Record<MochaToken, string> = {
  crust: rosePineMoonPalette.base,
  mantle: rosePineMoonPalette.surface,
  base: rosePineMoonPalette.base,
  surface0: rosePineMoonPalette.overlay,
  surface1: rosePineMoonPalette.highlightMed,
  surface2: rosePineMoonPalette.highlightHigh,
  overlay0: rosePineMoonPalette.readableMuted,
  subtext0: rosePineMoonPalette.readableMuted,
  text: rosePineMoonPalette.text,
  mauve: rosePineMoonPalette.iris,
  blue: rosePineMoonPalette.foam,
  green: rosePineMoonPalette.pine,
  yellow: rosePineMoonPalette.gold,
  red: rosePineMoonPalette.love,
  peach: rosePineMoonPalette.rose,
};

export function conversationThemeStyle(): Record<string, string> {
  const style: Record<string, string> = {};
  for (const [token, value] of Object.entries(conversationSurfaceTokens) as [MochaToken, string][]) {
    style[cssTokenName(token)] = value;
  }
  // Pre-existing conversation CSS references --ctp-overlay1, which Mocha never defined.
  style["--ctp-overlay1"] = rosePineMoonPalette.readableMuted;
  return style;
}

export function applyThemeTokens(target: CSSStyleDeclaration): void {
  for (const [token, value] of Object.entries(mochaPalette) as [MochaToken, string][]) {
    target.setProperty(cssTokenName(token), value);
  }
}

/** Monaco consumes this adapter rather than maintaining a second palette. */
export const monacoMochaTheme = {
  base: "vs-dark",
  inherit: true,
  colors: {
    "editor.background": mochaPalette.base,
    "editor.foreground": mochaPalette.text,
    "editorLineNumber.foreground": mochaPalette.overlay0,
    "editorLineNumber.activeForeground": mochaPalette.subtext0,
    "editor.selectionBackground": mochaPalette.surface2,
    "editorCursor.foreground": mochaPalette.mauve,
    "editorWidget.background": mochaPalette.mantle,
    "editorWidget.border": mochaPalette.surface1,
  },
} as const;
