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
 * 把一段 HTML 送去打印（浏览器面板里有「另存为 PDF」）。
 *
 * ## 为什么绕 iframe，而不是直接 `window.print()`
 *
 * `window.print()` 印的是**当前这个文档** —— 也就是整个应用界面：标题栏、
 * 标签栏、悬浮按钮全都在。用户想要的是笔记的正文，不是截图整个软件。
 *
 * 所以造一个隐藏的 `<iframe>`，把打印版 HTML 装进去，**在它里面**调 `print()`。
 * 印的就只有那段内容。
 *
 * ## 为什么样式要内联在 HTML 里
 *
 * iframe 的内容是 `srcdoc`，它**不继承**宿主文档的任何样式 —— 于是打印版那份
 * 自带的 `<style>`（见后端 `platform/print`）正是唯一能让它排得像样的东西。
 * 反过来说：如果把正文直接塞进主文档再打印，就得跟整套主题变量较劲，还清不掉。
 *
 * ## 用户的动作在这一步
 *
 * 浏览器会弹出打印面板，**由用户选「另存为 PDF」**。程序不替他决定，也不假装
 * 已经存好了 —— 界面那句提示就是这么说的（见 {@link printHtmlToPdf}）。
 */

import { invoke } from "@tauri-apps/api/core";
import { flash } from "../core/notice.ts";

/** 打印用的 iframe 留在文档里（隐藏），好让打印完那一下还能收到它的事件 */
let frame: HTMLIFrameElement | null = null;

/**
 * 让用户打印一篇笔记 —— 拿打印版 HTML，交给浏览器的打印。
 *
 * 返回 `false` 表示"用户取消或失败了"，调用方据此不给"已导出"那种提示。
 */
export async function printNoteAsPdf(
    title: string,
    reference: string | null = null,
): Promise<boolean> {
    let html: string;
    try {
        // 这一条**不需要路径**：存到哪儿由浏览器自己的保存对话框决定，
        // 所以后端返回的是 HTML 而不是"已保存的路径"。
        html = await invoke<string>("note_print_html", { title, reference });
    } catch (error) {
        flash(`准备打印失败：${error}`);
        return false;
    }

    printHtml(html);
    flash(`已打开打印面板：在里面选「另存为 PDF」就能存成文件（${title}）`);
    return true;
}

/**
 * 在一个隐藏 iframe 里打印这段 HTML。
 *
 * 独立出来是为了能被单独测试（它只碰 DOM，不碰 Tauri）。
 */
export function printHtml(html: string): void {
    // 上一张还在就换掉它：一个应用里连着打印两篇时，不然会留两个隐形文档
    frame?.remove();

    frame = document.createElement("iframe");
    // `display: none` 的 iframe 在部分 WebView 里不会布局，打印会得到空白页，
    // 所以挪出视口而不是藏掉 —— 内容照样不可见
    frame.setAttribute("aria-hidden", "true");
    frame.style.cssText = [
        "position: fixed",
        "right: 0",
        "bottom: 0",
        "width: 1px",
        "height: 1px",
        "border: 0",
        "visibility: hidden",
        "opacity: 0",
        "pointer-events: none",
    ].join("; ");
    frame.srcdoc = html;

    frame.addEventListener("load", () => {
        try {
            // 必须在 iframe 自己的 window 上调：印的是那个文档
            frame?.contentWindow?.focus();
            frame?.contentWindow?.print();
        } catch (error) {
            flash(`打开打印面板失败：${error}`);
        }
    });

    document.body.appendChild(frame);
}
