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
 * 选项卡（`::tabs`）的切换。
 *
 * 结构由后端渲染：一排标签（`.tabs__tab`，第一节带 `--on`）+ 每节一个面板
 * （`.tabs__panel`，没选中的带 `hidden`）。这里只接上「点标签换面板」这一件事 ——
 * 渲染器往输出里塞不了 `onclick`（塞了也过不了 CSP），而阅读视图与编辑器预览是
 * 两处注入，行为得由一处说了算。
 */

/** 数列，认不出就是 0 —— 标签上的 `data-tab` 是后端给的，缺了也不该把面板全藏起来 */
function tabIndex(tab: HTMLElement): number {
    const value = Number(tab.dataset.tab);
    return Number.isInteger(value) && value >= 0 ? value : 0;
}

/** 给正文里每个选项卡接上切换。可以反复调用：接过线的不会再接一遍。 */
export function wireTabs(root: HTMLElement): void {
    for (const tabs of root.querySelectorAll<HTMLElement>(".tabs")) {
        if (tabs.dataset.tabsReady) {
            continue;
        }
        tabs.dataset.tabsReady = "yes";

        // `:scope >` 是必须的：面板里还可能**再嵌**一个选项卡，
        // 不加这一层就会把里层的标签也算进来，两层的下标当场对不上
        const labels = [
            ...tabs.querySelectorAll<HTMLButtonElement>(":scope > .tabs__bar > .tabs__tab"),
        ];
        const panels = [
            ...tabs.querySelectorAll<HTMLElement>(":scope > .tabs__panel"),
        ];
        if (labels.length === 0) {
            continue;
        }

        const show = (index: number) => {
            labels.forEach((tab, at) => {
                const on = at === index;
                tab.classList.toggle("tabs__tab--on", on);
                tab.setAttribute("aria-selected", on ? "true" : "false");
            });
            panels.forEach((panel, at) => {
                if (at === index) {
                    panel.removeAttribute("hidden");
                } else {
                    panel.setAttribute("hidden", "hidden");
                }
            });
        };

        labels.forEach((tab) => {
            tab.addEventListener("click", () => show(tabIndex(tab)));
        });

        // 开局把状态摆正：后端已经把第一节标成选中，这里照它对齐面板
        const selected = labels.find((tab) => tab.classList.contains("tabs__tab--on"));
        show(selected ? tabIndex(selected) : 0);
    }
}
