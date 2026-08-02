import eslint from "@eslint/js";
import vue from "eslint-plugin-vue";
import typescript from "typescript-eslint";
import vueParser from "vue-eslint-parser";

export default typescript.config(
  {
    ignores: ["dist/**", "src-tauri/**", "node_modules/**"],
  },
  eslint.configs.recommended,
  ...vue.configs["flat/recommended"],
  ...typescript.configs.recommended,
  {
    files: ["**/*.vue"],
    languageOptions: {
      parser: vueParser,
      parserOptions: {
        parser: typescript.parser,
        ecmaVersion: "latest",
        sourceType: "module",
      },
    },
  },
  {
    files: ["**/*.{ts,vue}", "**/*.mjs", "**/*.js"],
    languageOptions: {
      parserOptions: {
        parser: typescript.parser,
        extraFileExtensions: [".vue"],
      },
    },
    rules: {
      "no-undef": "off",
      "vue/multi-word-component-names": "off",
      "vue/no-v-html": "error",
      "vue/max-attributes-per-line": "off",
      "vue/singleline-html-element-content-newline": "off",
      "vue/html-self-closing": "off",
      "@typescript-eslint/no-explicit-any": "error",
      "no-restricted-imports": [
        "error",
        {
          patterns: [
            {
              group: ["@tauri-apps/*"],
              message:
                "Direct Tauri API imports are forbidden outside src/bridge/. Use the DesktopBridge interface instead.",
            },
          ],
        },
      ],
    },
  },
  {
    files: ["src/bridge/**/*.ts"],
    rules: {
      "no-restricted-imports": "off",
    },
  },
);
