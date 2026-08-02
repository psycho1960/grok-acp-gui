import { createApp } from "vue";
import { createPinia } from "pinia";
import App from "../App.vue";
import "../shared/theme/tokens.css";
import { applyThemeTokens } from "../shared/theme/tokens";

export function mountApplication(selector = "#app"): void {
  applyThemeTokens(document.documentElement.style);
  createApp(App).use(createPinia()).mount(selector);
}
