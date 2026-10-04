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
 * 刷新：`useNavigation.refresh()` 走哪条路，以及 `reload` 动作叫什么。
 *
 * ## 为什么只有这几条能测
 *
 * 按钮**摆在标题栏的哪个位置**（桌面前进与首页之间、手机最左）测不了 ——
 * 那在 `.vue` 模板里，而这里没有 DOM、也没有服务端渲染器（`@vue/server-renderer`
 * 不在依赖里，而"零新依赖"是这个仓库测试的规矩）。
 * 把整段按钮抽成数据再 `v-for` 渲染倒是能测，但那是动一个**改坏了看不出来**的组件
 * （`WindowTitleBar.vue` 里有一整段注释在讲它上一次怎么被改坏的），
 * 而这个改动本身用户一眼就能看到。所以那里的位置是**人眼验**的，这里只钉行为。
 *
 * 行为里真正会静默错掉的是"刷新用哪条路"：`push` 与 `replace` 差一点点，
 * 表现是"刷新之后按后退，退回到了刷新之前那一模一样的页面" —— 很怪，但不会报错。
 */

import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { ref } from "vue";

import { useNavigation } from "../../src/composables/useNavigation.ts";
import type { Movement, TabState } from "../../src/core/tabs.ts";

/** 记下 `navigate` 被怎么调过的 */
function harness(committed = "卡片") {
    const calls: { input: string; movement?: Movement }[] = [];
    const navigation = useNavigation({
        active: ref(null) as never,
        committed: ref(committed) as never,
        canGoBack: ref(true) as never,
        navigate: async (input: string, movement?: Movement) => {
            calls.push({ input, movement });
            return true;
        },
        newTabWith: async () => {},
        goBack: async () => {},
    });
    return { navigation, calls };
}

describe("刷新走哪条路", () => {
    it("重新解析的是**当前那个地址**，不是写死的某一页", () => {
        // 写死的话在"全部页面"上刷新会去别处 —— 那是刷新按钮最典型的翻车方式
        const { navigation, calls } = harness("卡片");
        navigation.refresh();
        assert.deepEqual(calls, [{ input: "卡片", movement: "replace" }]);
    });

    it("用 replace：刷新不算一次跳转", () => {
        // `push` 的后果：刷新之后按后退，退回到刷新之前那一模一样的页面
        const { navigation, calls } = harness();
        navigation.refresh();
        assert.equal(calls[0]?.movement, "replace");
    });

    it("当前是特殊页也照刷（`special:` 那些不是笔记，标题栏照样摆着刷新）", () => {
        const { navigation, calls } = harness("special:all");
        navigation.refresh();
        assert.equal(calls[0]?.input, "special:all");
    });
});

describe("reload 动作的名字", () => {
    it("叫「刷新」，与标题栏那颗按钮同名", async () => {
        // 以前这个动作是整个窗口重载，所以叫"重载"；现在它与那颗按钮是同一件事。
        // 两处不同名会让人以为是两件事 —— 而设置页里显示的是这一处的名字。
        const { ACTIONS } = await import("../../src/core/keymap.ts");
        const reload = ACTIONS.find((action) => action.id === "reload");
        assert.ok(reload, "reload 这个动作不见了");
        assert.equal(reload.label, "刷新");
    });

    it("仍然绑在 Ctrl+R 上，且与同组的导航动作并列", async () => {
        const { ACTIONS } = await import("../../src/core/keymap.ts");
        const reload = ACTIONS.find((action) => action.id === "reload");
        assert.deepEqual(reload?.keys, ["Ctrl", "R"]);
        // 与前进/后退/首页/菜单同一组 —— 它们是"在这一页里进出"这一族
        assert.equal(reload?.group, "页面");
    });
});

// `TabState` 只是为了让上面的 import 有用处（`navigate` 的签名引用它）；
// 留着未使用的 import 会让类型检查报 TS6133，所以用它做个类型上的锚。
export type _TabStateAnchor = TabState;