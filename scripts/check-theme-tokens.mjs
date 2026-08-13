import { readFile, readdir } from "node:fs/promises";
import { join } from "node:path";

const palette = /#(?:11111b|181825|1e1e2e|313244|45475a|585b70|6c7086|a6adc8|cdd6f4|cba6f7|89b4fa|a6e3a1|f9e2af|f38ba8|fab387|232136|2a273f|393552|6e6a86|908caa|e0def4|c4a7e7|eb6f92|9ccfd8|f6c177|3e8fb0|ea9a97|44415a|56526e)/i;
const roots = ["src/app", "src/features", "src/shared/ui"];

for (const root of roots) {
  for (const entry of await readdir(root, { recursive: true })) {
    if (!entry.endsWith(".ts") && !entry.endsWith(".vue") && !entry.endsWith(".css")) continue;
    const file = join(root, entry);
    if (palette.test(await readFile(file, "utf8"))) throw new Error(`Mocha palette hex must use a theme token: ${file}`);
  }
}
