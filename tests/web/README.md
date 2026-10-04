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
 * 前端单元测试怎么跑、为什么这么跑。
 *
 * ## 跑法
 *
 *     pnpm test          # node --test tests/web/
 *
 * ## 为什么用 Node 自带的 `node:test`，不装 vitest / jest
 *
 * 三条理由，按分量排序：
 *
 * 1. **不引依赖**。这个仓库的前端依赖已经不少（CodeMirror 六个包、katex、mermaid…），
 *    而测试要用到的能力——跑 `.ts` 文件、断言、隔离每个文件——Node 22 起全自带了。
 *    为了跑 20 个纯函数的测试再拖进来一整套 runner 与其生态，不划算。
 * 2. **不引第二套解析器**。`.ts` 直接由 Node 剥类型（不是编译），
 *    所以测试跑的**就是源码本身**，不存在"测试里那份转译产物与源码不一致"。
 * 3. **失败得清楚**。`node --test` 直接打出哪个断言、实际值是什么，
 *    不需要 sourcemap 才看得出行号。
 *
 * 代价是明确的：**组件测不了**（要 DOM、要 Vue 挂载器）。
 * 所以这一层只测 `core/` 与 `core/markdown/` 里的**纯逻辑**——
 * 而那恰好是这个项目最容易悄悄算错的部分（模板块边界、键位归一、地址回显）。
 * 要测组件得引 `@vue/test-utils` + `happy-dom`，那是另一笔账，值得单独算。
 *
 * ## 放在 `tests/web/` 而不是 `src/` 旁边
 *
 * `src/` 里的每个文件都被打进前端产物；测试不该跟着走。
 * 目录名带 `web/` 是为了以后要测 Rust 侧时有个对称的位置（Rust 的测试是行内的）。
 */