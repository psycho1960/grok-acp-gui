import assert from "node:assert/strict";
import { readFile, readdir } from "node:fs/promises";
import test from "node:test";

const tokenSource = await readFile("src/shared/theme/tokens.ts", "utf8");
const cssSource = await readFile("src/shared/theme/tokens.css", "utf8");
const shellSource = await readFile("src/app/AppShell.vue", "utf8");
const uiDirectory = await readdir("src/shared/ui");

test("GAG-002 keeps the exact Catppuccin Mocha palette in one TypeScript source", () => {
  const expected = {
    crust: "#11111b", mantle: "#181825", base: "#1e1e2e", surface0: "#313244",
    surface1: "#45475a", surface2: "#585b70", overlay0: "#6c7086", subtext0: "#a6adc8",
    text: "#cdd6f4", mauve: "#cba6f7", blue: "#89b4fa", green: "#a6e3a1",
    yellow: "#f9e2af", red: "#f38ba8", peach: "#fab387",
  };
  for (const [name, hex] of Object.entries(expected)) assert.match(tokenSource, new RegExp(`${name}:\\s*["']${hex}["']`));
  assert.match(tokenSource, /monacoMochaTheme/);
  assert.doesNotMatch(cssSource, /#[0-9a-f]{6}/i);
});

test("GAG-002 supplies the required accessible primitive component set", async () => {
  for (const component of ["Button.vue", "IconButton.vue", "Input.vue", "Textarea.vue", "Select.vue", "Dialog.vue", "Drawer.vue", "Tooltip.vue", "Badge.vue", "StatusIcon.vue", "EmptyState.vue", "ErrorState.vue", "Skeleton.vue"]) assert.equal(uiDirectory.includes(component), true, component);
  assert.match(await readFile("src/shared/ui/Dialog.vue", "utf8"), /aria-modal="true"/);
  assert.match(await readFile("src/shared/ui/Dialog.vue", "utf8"), /Escape/);
  assert.match(await readFile("src/shared/ui/Drawer.vue", "utf8"), /Escape/);
});

test("GAG-002 AppShell retains the specified slots, resizing bounds, and responsive fallback", () => {
  for (const text of ["export type AppShellProps", "left: VNode", "main: VNode", "inspectorOpen: boolean", "Math.min(360, Math.max(220", "minmax(520px, 1fr)", "max-width: 1200px", "min-resolution: 1.75dppx"]) assert.match(shellSource, new RegExp(text.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")));
});

test("GAG-002 lint gate rejects Mocha palette hex outside the theme source", async () => {
  const files = [];
  for (const root of ["src/app", "src/features", "src/shared/ui"]) {
    for (const file of await readdir(root, { recursive: true })) files.push(`${root}/${file}`);
  }
  const palette = /#(?:11111b|181825|1e1e2e|313244|45475a|585b70|6c7086|a6adc8|cdd6f4|cba6f7|89b4fa|a6e3a1|f9e2af|f38ba8|fab387)/i;
  for (const file of files.filter((entry) => entry.endsWith(".vue") || entry.endsWith(".css") || entry.endsWith(".ts"))) {
    const source = await readFile(file, "utf8");
    assert.doesNotMatch(source, palette, file);
  }
});
