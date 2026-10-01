import { createApp } from "vue";
import App from "./App.vue";
import { openWorkspace } from "./preferences.ts";
// 全局样式要在挂载前就位，否则首帧会是浏览器默认的那套配色
import "./css/clean.css";
import "./css/theme.css";
import "./styles/note.css";
import "./styles/tooltip.css";

// 先把壳挂起来：启动还没跑完时主区域显示的是加载页（见 startup.ts）。
// 偏好读回来之后主题、主题色、缩放立刻应用（见 openWorkspace）。
createApp(App).mount("#app");
void openWorkspace();
