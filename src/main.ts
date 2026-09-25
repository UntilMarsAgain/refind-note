import { createApp } from "vue";
import App from "./App.vue";
import { initTheme } from "./theme";
import "./styles/theme.css";
import "./styles/note.css";

// 先定下主题再挂载，避免首帧闪一下默认配色
initTheme();

createApp(App).mount("#app");
