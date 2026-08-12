import assert from "node:assert/strict";
import { readFile, readdir } from "node:fs/promises";
import test from "node:test";

const cssSource = await readFile("src/shared/theme/tokens.css", "utf8");
const toastSource = await readFile("src/shared/ui/toast.ts", "utf8");
const statusIcon = await readFile("src/shared/ui/StatusIcon.vue", "utf8");
const breakpoints = await readFile("src/shared/composables/breakpoints.ts", "utf8");
const uiDirectory = await readdir("src/shared/ui");

test("GAG-017 defines typography, elevation, and breakpoint tokens", () => {
  for (const token of [
    "--text-xs",
    "--text-4xl",
    "--heading-page",
    "--heading-dialog",
    "--leading-tight",
    "--font-weight-semibold",
    "--shadow-sm",
    "--elevation-modal",
    "--radius-panel",
    "--backdrop-alpha",
    "--space-5",
    "--bp-xl",
  ]) {
    assert.match(cssSource, new RegExp(token.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")));
  }
  assert.match(breakpoints, /xl:\s*1200/);
  assert.match(breakpoints, /lg:\s*1080/);
});

test("GAG-017 ships Toast and SVG icon primitives", () => {
  for (const file of ["ToastHost.vue", "toast.ts", "Icon.vue", "NamedIcon.vue", "icons.ts"]) {
    assert.equal(uiDirectory.includes(file), true, file);
  }
  assert.match(toastSource, /success\(/);
  assert.match(toastSource, /error\(/);
  assert.match(statusIcon, /NamedIcon/);
  assert.doesNotMatch(statusIcon, /[◌✓!⌁◈]/);
});

test("GAG-017 App mounts ToastHost and AppShell uses NamedIcon", async () => {
  const app = await readFile("src/App.vue", "utf8");
  const shell = await readFile("src/app/AppShell.vue", "utf8");
  assert.match(app, /ToastHost/);
  assert.match(shell, /NamedIcon/);
  assert.doesNotMatch(shell, /☰|☷/);
  assert.doesNotMatch(shell, /font-weight:\s*650/);
});
