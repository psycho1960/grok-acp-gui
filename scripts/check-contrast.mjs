/**
 * WCAG contrast gate for Mocha (app chrome) and Rose Pine Moon (conversation).
 * Keep hex values aligned with src/shared/theme/tokens.ts.
 */
const mochaPalette = {
  crust: "#11111b",
  mantle: "#181825",
  base: "#1e1e2e",
  surface0: "#313244",
  subtext0: "#a6adc8",
  text: "#cdd6f4",
  mauve: "#cba6f7",
  blue: "#89b4fa",
  green: "#a6e3a1",
  yellow: "#f9e2af",
  red: "#f38ba8",
};

const rosePineMoonPalette = {
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
  readableMuted: "#b4b1cb",
};

function hexToRgb(hex) {
  const h = hex.replace("#", "");
  return {
    r: parseInt(h.slice(0, 2), 16) / 255,
    g: parseInt(h.slice(2, 4), 16) / 255,
    b: parseInt(h.slice(4, 6), 16) / 255,
  };
}

function channel(c) {
  return c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
}

function luminance(hex) {
  const { r, g, b } = hexToRgb(hex);
  return 0.2126 * channel(r) + 0.7152 * channel(g) + 0.0722 * channel(b);
}

function contrast(a, b) {
  const l1 = luminance(a);
  const l2 = luminance(b);
  const lighter = Math.max(l1, l2);
  const darker = Math.min(l1, l2);
  return (lighter + 0.05) / (darker + 0.05);
}

/** [fg, bg, minRatio, label] */
const pairs = [
  [mochaPalette.text, mochaPalette.base, 4.5, "body text on base"],
  [mochaPalette.text, mochaPalette.mantle, 4.5, "body text on mantle"],
  [mochaPalette.subtext0, mochaPalette.base, 3, "secondary text on base (UI/large)"],
  [mochaPalette.subtext0, mochaPalette.mantle, 3, "secondary text on mantle"],
  [mochaPalette.crust, mochaPalette.yellow, 4.5, "warning badge text"],
  [mochaPalette.crust, mochaPalette.red, 4.5, "danger badge text"],
  [mochaPalette.crust, mochaPalette.green, 3, "success on green (large)"],
  [mochaPalette.mauve, mochaPalette.base, 3, "primary accent on base"],
  [mochaPalette.blue, mochaPalette.base, 3, "info accent on base"],
  [rosePineMoonPalette.text, rosePineMoonPalette.base, 4.5, "conversation body on base"],
  [rosePineMoonPalette.text, rosePineMoonPalette.surface, 4.5, "conversation body on surface"],
  [rosePineMoonPalette.text, rosePineMoonPalette.overlay, 4.5, "conversation body on overlay"],
  [rosePineMoonPalette.readableMuted, rosePineMoonPalette.base, 4.5, "conversation secondary on base"],
  [rosePineMoonPalette.readableMuted, rosePineMoonPalette.surface, 4.5, "conversation secondary on surface"],
  [rosePineMoonPalette.readableMuted, rosePineMoonPalette.overlay, 4.5, "conversation secondary on overlay"],
  [rosePineMoonPalette.iris, rosePineMoonPalette.base, 3, "conversation iris on base"],
  [rosePineMoonPalette.love, rosePineMoonPalette.base, 3, "conversation love on base"],
  [rosePineMoonPalette.foam, rosePineMoonPalette.base, 3, "conversation foam on base"],
  [rosePineMoonPalette.gold, rosePineMoonPalette.base, 3, "conversation gold on base"],
  [rosePineMoonPalette.base, rosePineMoonPalette.gold, 4.5, "conversation warning badge text"],
  [rosePineMoonPalette.base, rosePineMoonPalette.love, 4.5, "conversation danger badge text"],
];

let failed = 0;
for (const [fg, bg, min, label] of pairs) {
  const ratio = contrast(fg, bg);
  const ok = ratio + 1e-6 >= min;
  console.log(
    `${ok ? "OK" : "FAIL"}  ${ratio.toFixed(2)}:1 (need ≥ ${min}) — ${label}`,
  );
  if (!ok) failed += 1;
}

if (failed) {
  console.error(`\nContrast check failed: ${failed} pair(s).`);
  process.exit(1);
}
console.log("\nContrast check passed.");
