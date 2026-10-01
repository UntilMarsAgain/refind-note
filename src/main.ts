/**
 * 前端入口。
 *
 * ## 目录怎么分
 *
 * 按"这一层**认识什么**"分，改的时候别把边界搅乱：
 *
 * - `ipc/` —— 与后端一一对应的**类型**：这些形状由 Rust 说了算，前端照着用；
 * - `core/` —— 状态与纯逻辑：不认识 DOM，也不认识组件（`core/markdown/` 是解析口径）；
 * - `dom/` —— HTML 注入 DOM 之后的收尾与交互：高亮、右键、播放器、编辑器配置……
 * - `components/` —— 组件：`shell/` 外壳、`note/` 笔记、`pages/` 特殊页面、`common/` 共用小件；
 * - `styles/` —— 全部样式。
 *
 * 后端那边的分法写在 `src-tauri/src/lib.rs` 的抬头里，两边的层次是对着的。
 */

import { createApp } from "vue";
import App from "./App.vue";
import { openWorkspace } from "./core/preferences.ts";
// 全局样式要在挂载前就位，否则首帧会是浏览器默认的那套配色
import "./styles/base.css";
import "./styles/theme.css";
import "./styles/note.css";
import "./styles/tooltip.css";

// 先把壳挂起来：启动还没跑完时主区域显示的是加载页（见 startup.ts）。
// 偏好读回来之后主题、主题色、缩放立刻应用（见 openWorkspace）。
createApp(App).mount("#app");
void openWorkspace();
