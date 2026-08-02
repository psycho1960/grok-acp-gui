import { readFile, readdir } from "node:fs/promises";
import { join } from "node:path";

const palette = /#(?:11111b|181825|1e1e2e|313244|45475a|585b70|6c7086|a6adc8|cdd6f4|cba6f7|89b4fa|a6e3a1|f9e2af|f38ba8|fab387)/i;
const roots = ["src/app", "src/features", "src/shared/ui"];

for (const root of roots) {
  for (const entry of await readdir(root, { recursive: true })) {
    if (!entry.endsWith(".ts") && !entry.endsWith(".vue") && !entry.endsWith(".css")) continue;
    const file = join(root, entry);
    if (palette.test(await readFile(file, "utf8"))) throw new Error(`Mocha palette hex must use a theme token: ${file}`);
  }
}
