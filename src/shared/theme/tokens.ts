export const mochaPalette = {
  crust: "#11111b", mantle: "#181825", base: "#1e1e2e",
  surface0: "#313244", surface1: "#45475a", surface2: "#585b70",
  overlay0: "#6c7086", subtext0: "#a6adc8", text: "#cdd6f4",
  mauve: "#cba6f7", blue: "#89b4fa", green: "#a6e3a1",
  yellow: "#f9e2af", red: "#f38ba8", peach: "#fab387",
} as const;

export type MochaToken = keyof typeof mochaPalette;

export const cssTokenName = (token: MochaToken): string => `--ctp-${token}`;

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
