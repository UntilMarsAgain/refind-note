#!/usr/bin/env python3
"""一条命令跑完所有自检：格式、静态检查、测试、类型。

## 为什么要有这个脚本

这个仓库有**三套**工具链同时管着同一份代码，历史上已经反复栽在
"只跑了其中一套、另一套早就坏了"上：

- **Rust**：`cargo fmt` / `cargo clippy` / `cargo test`；
- **TypeScript**：`vue-tsc --noEmit`（`pnpm build` 的第一步）；
- **仓库结构**：通用依赖有没有被误划进 `[target.*]`（下面 `check_target_deps()`）。

没有 CI，也没有 Makefile，于是"改完到底该跑哪几条"这件事全靠记性。
本项目已经因为这个丢过东西：`clippy` 攒了 6 条告警没人清，
`Cargo.toml` 的表头把通用依赖划进桌面段（提交 `61b1f7c` 才发现）。
原来那份检查是个单独脚本 `check_target_deps.py`，后来删掉了 ——
所以并进本文件，它跟着这些步骤一起跑，不至于有人先删脚本再忘了这回事。

所以这里把该跑的收在一处：**跑一遍就知道这个提交是不是干净的**，
而且每一步都用机器判，不靠人眼数。

## 用法

    python3 selfcheck.py              # 全跑一遍
    python3 selfcheck.py --fast       # 跳过慢的那几条（clippy / test）
    python3 selfcheck.py --only fmt ts   # 只跑这几条（名字见 STEPS）

退出码 0 = 全过；非 0 = 有东西没过（有几条不过就是几）。

## 关于「不跑 UI 实测」

这里**故意不跑** `tauri dev`、`tauri build`、任何 `pnpm dev`：
那几样要弹窗口、要人点，无人值守时会把进程挂死。
它们验的是"程序起不起得来"，而这里要验的是"代码自洽不自洽"——
后者不需要窗口，也就不需要人。
"""

from __future__ import annotations

import argparse
import re
import shutil
import subprocess
import sys
import time
from dataclasses import dataclass
from pathlib import Path

ROOT = Path(__file__).resolve().parent

# 单条命令的上限（秒）。超时算**失败**：卡住与失败一样是"这一步没成"，
# 只是更难看 —— 所以给它一个明确的失败而不是让整个脚本挂在那里。
TIMEOUT = 900


@dataclass(frozen=True)
class Step:
    """一步自检。"""

    name: str
    #: 干什么用的（失败时打印出来，让人知道少验了什么）
    what: str
    argv: list[str]
    #: 慢的那几条（`--fast` 跳过）
    slow: bool = False
    #: 不跑外部命令、直接在 Python 里算的检查（`argv` 为空时用它）
    built_in: "Callable[[], tuple[bool, str]] | None" = None


def has(program: str) -> bool:
    """这个工具装了没有（`--fast` 之类不该因为缺个可选工具就整体失败）"""
    return shutil.which(program) is not None


def optional(name: str, what: str, argv: list[str]) -> Step:
    """一步自检；命令不存在时跳过而不是报错。"""
    return Step(name, what, argv, False)


STEPS: list[Step] = [
    Step(
        "deps",
        "通用依赖有没有被误划进 [target.*]（那样它只对桌面生效，Android 编不过）",
        [],
        built_in=lambda: check_target_deps(),
    ),
    Step(
        "fmt",
        "Rust 排版（rustfmt 的默认风格）",
        ["cargo", "fmt",  "--", "--check"],
    ),
    Step(
        "clippy",
        "Rust 静态检查（含测试代码），告警一律当错",
        [
            "cargo",
            "clippy",
            "--manifest-path",
            str(ROOT / "src-tauri" / "Cargo.toml"),
            "--all-targets",
            "--",
            "-D",
            "warnings",
        ],
        slow=True,
    ),
    Step(
        "rust-test",
        "Rust 单元测试（地址、封装、模板块、同步判定、键位……）",
        ["cargo", "test"],
        slow=True,
    ),
    optional(
        "ts",
        "前端类型检查（vue-tsc，含 .vue 里的模板）",
        ["pnpm", "exec", "vue-tsc", "--noEmit"],
    ),
    optional(
        "web-test",
        "前端单元测试（node:test，不需要浏览器）",
        ["node", "--test", "tests/web/"],
    ),
]


def run(step: Step) -> tuple[bool, str]:
    """跑一步。返回（过不过、过的话输出是什么）。"""
    if step.built_in is not None:
        started = time.monotonic()
        ok, detail = step.built_in()
        if ok:
            return True, f"{time.monotonic() - started:.1f} 秒　{detail}"
        return False, detail

    if not has(step.argv[0]):
        return True, f"跳过：没装 {step.argv[0]}"

    started = time.monotonic()
    try:
        done = subprocess.run(
            step.argv,
            cwd=ROOT,
            timeout=TIMEOUT,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            text=True,
            errors="replace",
        )
    except subprocess.TimeoutExpired:
        return False, f"超过 {TIMEOUT} 秒还没完，当作失败"
    except OSError as error:
        return False, f"跑不起来：{error}"

    spent = time.monotonic() - started
    if done.returncode != 0:
        return False, f"退出码 {done.returncode}\n{tail_of(done.stdout)}"
    return True, f"{spent:.1f} 秒{summary_of(done.stdout)}"


# 过的时候只留"过了多少" —— 几百行编译输出里没有别的信息。
# 两种 runner 的计数长得不一样，分开认：
#   cargo test → `test result: ok. 310 passed; 0 failed; …`
#   node --test → `ℹ tests 99` / `ℹ pass 99` / `ℹ fail 0`
CARGO_COUNT = re.compile(r"test result: \w+\. (\d+) passed; (\d+) failed")
NODE_COUNT = re.compile(r"^. (pass|fail|tests) (\d+)$", re.MULTILINE)


def summary_of(text: str) -> str:
    """通过时给出**计数**那几行（`310 passed`、`tests 94`）。

    没有计数行那就给全部 —— 但只有一两句时才给，几百行仍然丢掉。
    """
    lines = [line.strip() for line in text.splitlines() if line.strip()]

    # `cargo test` 的计数（空的 target 会报 "0 passed"，那种不用报）
    rust = CARGO_COUNT.findall(text)
    if rust:
        passed = sum(int(p) for p, _ in rust)
        failed = sum(int(f) for _, f in rust)
        parts = [f"Rust {passed} 通过"]
        if failed:
            parts.append(f"{failed} 失败")
        return f"（{' / '.join(parts)}）"

    # `node --test` 的计数
    node = {name: int(value) for name, value in NODE_COUNT.findall(text)}
    if node.get("tests") is not None:
        return f"（前端 {node['tests']} 条 / 过 {node.get('pass', 0)} / 败 {node.get('fail', 0)}）"

    # 认不出计数：输出很短就全给，几百行仍然丢掉
    if 0 < len(lines) <= 3:
        return f"（{' / '.join(lines)}）"
    return ""


def tail_of(text: str, limit: int = 24) -> str:
    """失败时只留最后几行 —— 几百行编译输出没人看，但末尾那几行是原因。"""
    lines = [line for line in text.splitlines() if line.strip()]
    if not lines:
        return ""
    picked = lines[-limit:]
    prefix = "（只显示最后 24 行）\n" if len(lines) > limit else ""
    return prefix + "\n".join(picked)


def check_target_deps() -> tuple[bool, str]:
    """通用依赖有没有被误划进 `[target.*]`。

    ## 为什么要查

    TOML 里**一旦出现新的表头，它后面的行就都归那一张表**。所以把
    `[target.*.dependencies]` 写在 `[dependencies]` 中间，会让后面的通用依赖
    静默地只对桌面生效 —— `Cargo.toml` 看上去完全正常，桌面构建也完全正常，
    只有编 Android 时才炸（`cannot find module or crate`）。

    本项目栽过两次（一次吞进移动端段、一次留在桌面段），所以机器查一遍。
    """
    import re

    manifest = ROOT / "src-tauri" / "Cargo.toml"
    # 这些确实该在某个 target 段里（平台专有），不算错
    allowed = {"gpgme", "tauri-plugin-dialog"}

    table = "[dependencies]"
    offenders: list[str] = []
    for number, line in enumerate(manifest.read_text().splitlines(), 1):
        header = re.match(r"^\[([^\]]+)\]", line)
        if header:
            table = header.group(1)
            continue
        # **只管 `[target.*]`**：其余的表（package / lib / build-dependencies /
        # profile.release …）里出现 `名字 = 值` 是天经地义的，不是错。
        # 我第一版漏了这个条件，于是把 [package] 里的 name、version 全报成"落在平台段"。
        if not table.startswith("target."):
            continue
        entry = re.match(r"^([A-Za-z0-9_-]+)\s*=", line)
        if not entry or entry.group(1) in allowed:
            continue
        offenders.append(f"第 {number} 行：{entry.group(1)}（落在 [{table}] 里）")

    if offenders:
        return False, "通用依赖落在了平台段里：\n    " + "\n    ".join(offenders)
    return True, "通用依赖都在 [dependencies] 里"


def main() -> int:
    parser = argparse.ArgumentParser(description="跑一遍这个仓库该跑的自检")
    parser.add_argument("--fast", action="store_true", help="跳过慢的那几条（clippy / rust-test）")
    parser.add_argument(
        "--only",
        metavar="名字",
        help="只跑指定的几条，逗号分隔（名字见脚本里的 STEPS）",
    )
    args = parser.parse_args()

    wanted = None
    if args.only:
        wanted = {name.strip() for name in args.only.split(",") if name.strip()}
        unknown = wanted - {step.name for step in STEPS}
        if unknown:
            print(f"没有这几步：{'、'.join(sorted(unknown))}", file=sys.stderr)
            print(f"可选的：{'、'.join(step.name for step in STEPS)}", file=sys.stderr)
            return 2

    steps = [step for step in STEPS if wanted is None or step.name in wanted]
    if args.fast:
        steps = [step for step in steps if not step.slow]

    if not steps:
        print("没有要跑的东西。", file=sys.stderr)
        return 2

    print(f"自检：{len(steps)} 步\n")
    failed: list[Step] = []
    for step in steps:
        print(f"—— {step.name}：{step.what}")
        ok, detail = run(step)
        print(f"   {'通过' if ok else '不过'}{'  ' + detail if detail else ''}\n")
        if not ok:
            failed.append(step)

    if not failed:
        print("全过。")
        return 0

    print("没过：")
    for step in failed:
        print(f"  {step.name}：{step.what}")
    print("\n单独重跑其中一条：python3 selfcheck.py --only " + ",".join(s.name for s in failed))
    return len(failed)


if __name__ == "__main__":
    sys.exit(main())