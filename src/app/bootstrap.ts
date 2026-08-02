import { createApp } from "vue";
import { createPinia } from "pinia";
import App from "../App.vue";
import "../shared/theme/tokens.css";

export function mountApplication(selector = "#app"): void {
  createApp(App).use(createPinia()).mount(selector);
}
