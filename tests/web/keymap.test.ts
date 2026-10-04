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
 * `core/keymap.ts`：键位的识别与显示。
 *
 * 只测**纯函数**那几半（`pressedKeys` / `matches` / `label` / 表的查询）。
 * 落盘那半要问后端，不在这一层。
 */

import assert from "node:assert/strict";
import { describe, it } from "node:test";

import {
    ACTIONS,
    SHORTCUT_GROUPS,
    actionById,
    actionsInGroup,
    isCustomized,
    label,
    matches,
    pressedKeys,
} from "../../src/core/keymap.ts";

/** 造一个 `KeyboardEvent` 的替身：只要那几个被读的字段 */
function key(
    name: string,
    modifiers: { ctrl?: boolean; shift?: boolean; alt?: boolean; meta?: boolean } = {},
): KeyboardEvent {
    return {
        key: name,
        ctrlKey: Boolean(modifiers.ctrl),
        shiftKey: Boolean(modifiers.shift),
        altKey: Boolean(modifiers.alt),
        metaKey: Boolean(modifiers.meta),
        getModifierState: () => false,
    } as unknown as KeyboardEvent;
}

describe("一次按键算成什么", () => {
    it("修饰键在前，主键在后", () => {
        assert.deepEqual(pressedKeys(key("T", { ctrl: true })), ["Ctrl", "T"]);
        assert.deepEqual(pressedKeys(key("F3", { ctrl: true, shift: true })), [
            "Ctrl",
            "Shift",
            "F3",
        ]);
    });

    it("空格写作 `Space`、`Escape` 写作 `Esc`（与存盘时同一套写法）", () => {
        assert.deepEqual(pressedKeys(key(" ")), ["Space"]);
        assert.deepEqual(pressedKeys(key("Escape")), ["Esc"]);
    });

    it("单个字母按大小写都算同一个键", () => {
        assert.deepEqual(pressedKeys(key("a")), ["A"]);
        assert.deepEqual(pressedKeys(key("A")), ["A"]);
    });

    it("只按着修饰键不算一次按键（否则会算成 `Ctrl+Ctrl`）", () => {
        assert.equal(pressedKeys(key("Control", { ctrl: true })), null);
        assert.equal(pressedKeys(key("Shift", { shift: true })), null);
        assert.equal(pressedKeys(key("Alt")), null);
    });

    it("CapsLock 也算进修饰键", () => {
        const event = {
            key: "T",
            ctrlKey: true,
            shiftKey: false,
            altKey: false,
            metaKey: false,
            getModifierState: (name: string) => name === "CapsLock",
        } as unknown as KeyboardEvent;
        assert.deepEqual(pressedKeys(event), ["Ctrl", "CapsLock", "T"]);
    });
});

describe("匹配", () => {
    const find = () => ACTIONS.find((action) => action.id === "find");
    const findNext = () => ACTIONS.find((action) => action.id === "find-next");

    it("修饰键必须完全一致", () => {
        assert.ok(find());
        assert.equal(matches(key("F", { ctrl: true }), find()!.keys), true);
        // 少按一个 Shift 就不该命中 —— `Ctrl+F` 与 `Ctrl+Shift+F` 是两回事
        assert.equal(matches(key("F", { ctrl: true, shift: true }), find()!.keys), false);
    });

    it("多按一个没在键位里的修饰键也不命中", () => {
        assert.ok(find());
        assert.equal(matches(key("F", { ctrl: true, alt: true }), find()!.keys), false);
    });

    it("没有主键（只按着 Ctrl）不算命中", () => {
        assert.ok(find());
        assert.equal(matches(key("Control", { ctrl: true }), find()!.keys), false);
    });

    it("空键位谁也不匹配（表示这个动作没绑键）", () => {
        assert.equal(matches(key("F", { ctrl: true }), []), false);
    });

    it("无修饰键的键位就是裸键", () => {
        assert.ok(findNext());
        assert.equal(matches(key("F3"), findNext()!.keys), true);
        assert.equal(matches(key("F3", { ctrl: true }), findNext()!.keys), false);
    });
});

describe("显示", () => {
    it("修饰键用加号连起来", () => {
        assert.equal(label(["Ctrl", "Shift", "T"]), "Ctrl+Shift+T");
    });

    it("方向键显示成箭头", () => {
        assert.equal(label(["Alt", "ArrowLeft"]), "Alt+←");
        assert.equal(label(["Alt", "ArrowRight"]), "Alt+→");
    });

    it("空键位给空串（不是一个孤零零的加号）", () => {
        assert.equal(label([]), "");
    });
});

describe("动作表", () => {
    it("动作 id 不重复", () => {
        const ids = ACTIONS.map((action) => action.id);
        assert.equal(new Set(ids).size, ids.length);
    });

    it("出厂键位互不重叠（与 Rust 那边 `keymap.rs` 同一条规矩）", () => {
        const seen = new Map<string, string>();
        for (const action of ACTIONS) {
            const binding = action.keys.join("+");
            const owner = seen.get(binding);
            assert.equal(
                owner,
                undefined,
                `「${action.label}」与「${owner}」抢同一个键 ${binding}`,
            );
            seen.set(binding, action.label);
        }
    });

    it("键名不是已废弃的别名（写错就是永远按不出来）", () => {
        const deprecated = ["Left", "Right", "Up", "Down", "Esc", "Spacebar", "Del"];
        for (const action of ACTIONS) {
            for (const key of action.keys) {
                assert.equal(
                    deprecated.includes(key),
                    false,
                    `「${action.label}」用了已废弃的键名 ${key}`,
                );
            }
        }
    });

    it("分组按第一次出现的次序，不另维护一份", () => {
        const firstSeen: string[] = [];
        for (const action of ACTIONS) {
            if (!firstSeen.includes(action.group)) {
                firstSeen.push(action.group);
            }
        }
        assert.deepEqual(SHORTCUT_GROUPS, firstSeen);
    });

    it("每一组里的动作就是那一组的", () => {
        for (const group of SHORTCUT_GROUPS) {
            const inGroup = actionsInGroup(group);
            assert.ok(inGroup.length > 0, `${group} 里一个动作都没有`);
            assert.ok(inGroup.every((action) => action.group === group));
        }
    });

    it("按 id 找动作；不认识就给 null，不给假的", () => {
        assert.equal(actionById("find")?.id, "find");
        assert.equal(actionById("没有这个动作"), null);
    });
});

describe("改没改过", () => {
    it("没动过键位时不算改过（'恢复默认'不该可点）", () => {
        const find = ACTIONS.find((action) => action.id === "find");
        assert.ok(find);
        // 运行时的生效表此刻是出厂那份（`loadKeymap` 还没跑）
        assert.equal(isCustomized(find), false);
    });
});