//   Refind Note is a note-taking software.
//   Copyright (C) 2026 Until Mars Again
//
//   This program is free software: you can redistribute it and/or modify
//   it under the terms of the GNU Affero General Public License as published by
//   the Free Software Foundation, either version 3 of the License, or
//   (at your option) any later version.
//
//   This program is distributed in the hope that it will be useful,
//   but WITHOUT ANY WARRANTY; without even the implied warranty of
//   MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
//   GNU Affero General Public License for more details.
//
//   You should have received a copy of the GNU Affero General Public License
//   along with this program.  If not, see <http://www.gnu.org/licenses/>.

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
import { flash } from "./core/notice.ts";
// 全局样式要在挂载前就位，否则首帧会是浏览器默认的那套配色
import "./styles/base.css";
import "./styles/theme.css";
import "./styles/note.css";
import "./styles/tooltip.css";

const app = createApp(App);

/**
 * 渲染出错要说给人听。
 *
 * Vue 在渲染抛异常时会**保留上一次成功的 DOM** —— 于是界面看着好好的（停在最后一帧），
 * 而数据已经变了：地址栏换了页、内容却不动，点击也没反应。**这种失效最难查**，因为
 * 屏幕上什么都没有指出来。
 *
 * 所以这里把错误**摆到界面上**，而不只是打进控制台：至少能看见"出了什么事"，
 * 也能从提示里抄到去哪儿看详细。
 */
app.config.errorHandler = (error, _instance, info) => {
    console.error("[渲染出错]", info, error);
    flash(`界面出了点问题：${describeError(error)}`);
};

/**
 * 把一个错误说成人话。
 *
 * 原来直接 `String(error)`，于是**抛出来的东西不是 `Error` 时就变成
 * `[object Object]`** —— 恰好是最需要看清的那种情况（组件里抛普通对象、
 * 或抛出结构化的 `{ code, message }`）把信息全丢了，只剩一句"界面出了点问题"。
 *
 * 所以分几层往里挖，每一层都比上一层更像人话：
 * - `Error`：要 `message`，带上 `name`（`TypeError` 比"xxx is not a function"有用）
 * - `message` 字段的对象：直接取它（很多库抛的是 `{ message }`）
 * - 别的对象：挑几个常见字段（`code` / `reason` / `error`）再挖一层，
 *   都没有才退回 JSON —— JSON 至少比 `[object Object]` 能看出形状
 * - 都不是：`String(value)`
 */
function describeError(value: unknown): string {
    if (value instanceof Error) {
        return value.name && value.name !== "Error"
            ? `${value.name}: ${value.message}`
            : value.message;
    }
    if (value && typeof value === "object") {
        const record = value as Record<string, unknown>;
        for (const key of ["message", "reason", "code", "error"]) {
            const inner = record[key];
            if (typeof inner === "string" && inner) {
                return inner;
            }
            // `error` 常再包一层（`{ error: { message } }`），递归挖一次就够
            if (inner && typeof inner === "object") {
                const nested = describeError(inner);
                if (nested !== "[object Object]") {
                    return nested;
                }
            }
        }
        try {
            return JSON.stringify(value);
        } catch {
            return "（一个无法序列化的对象）";
        }
    }
    return String(value);
}

// 先把壳挂起来：启动还没跑完时主区域显示的是加载页（见 startup.ts）。
// 偏好读回来之后主题、主题色、缩放立刻应用（见 openWorkspace）。
app.mount("#app");
void openWorkspace();
