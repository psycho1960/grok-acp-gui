import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const tokenSource = await readFile("src/shared/theme/tokens.ts", "utf8");
const contrastSource = await readFile("scripts/check-contrast.mjs", "utf8");
const conversationView = await readFile("src/features/conversation/ConversationView.vue", "utf8");

test("GAG-021 keeps official Rose Pine Moon values in the token source", () => {
  const expected = {
    base: "#232136",
    surface: "#2a273f",
    overlay: "#393552",
    text: "#e0def4",
    iris: "#c4a7e7",
    love: "#eb6f92",
    foam: "#9ccfd8",
    gold: "#f6c177",
  };
  assert.match(tokenSource, /rosePineMoonPalette/);
  for (const [name, hex] of Object.entries(expected)) {
    assert.match(tokenSource, new RegExp(`${name}:\\s*["']${hex}["']`));
  }
  assert.match(tokenSource, /mochaPalette/);
  assert.match(tokenSource, /base:\s*["']#1e1e2e["']/);
});

test("GAG-021 conversation view consumes tokens rather than hardcoded hex", () => {
  assert.doesNotMatch(conversationView, /#[0-9a-f]{6}/i);
  assert.match(conversationView, /conversationThemeStyle/);
});

test("GAG-021 contrast gate covers conversation Rose Pine Moon pairs", () => {
  assert.match(contrastSource, /#232136/);
  assert.match(contrastSource, /#e0def4/);
  assert.match(contrastSource, /#c4a7e7/);
  assert.match(contrastSource, /#eb6f92/);
  assert.match(contrastSource, /#9ccfd8/);
  assert.match(contrastSource, /conversation/);
});
