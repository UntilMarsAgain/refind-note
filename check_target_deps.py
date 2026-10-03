#!/usr/bin/env python3
"""检查 Cargo.toml 里有没有把**通用依赖**误放进 `[target.*.dependencies]`。

## 为什么要有这个检查

TOML 里一旦出现新的表头，后面的行就都归那一张表。于是把桌面段写在
`[dependencies]` 中间，后面那些通用依赖（`ureq` / `aes-gcm` / …）
就会**静默地只对桌面生效** —— 桌面构建照过，Android 编译时报
`cannot find module or crate`，而 `Cargo.toml` 看上去完全正常。

这个坑不报错、只在小屏幕上炸，所以与其靠人眼数表头，不如让机器查。

## 查什么

凡是**没有** `[target.'cfg(...mobile...)]` 字样、却在某个
`[target.*]` 段里出现的依赖，都报出来 —— 除非它本来就该是平台专有的
（`gpgme`、`tauri-plugin-dialog` 这类，见下方的 ALLOWED）。

用法：
    python3 check_target_deps.py src-tauri/Cargo.toml
退出码 0 = 没问题，1 = 有通用依赖被划错段。
"""

import re
import sys

# 这些确实是平台专有的，出现在任何 target 段里都不算错。
ALLOWED = {"gpgme", "tauri-plugin-dialog"}

# 移动端段的判定：cfg 里出现 android 或 ios。
MOBILE_RE = re.compile(r"android|ios")


def main() -> int:
    path = sys.argv[1] if len(sys.argv) > 1 else "src-tauri/Cargo.toml"
    with open(path, encoding="utf-8") as handle:
        lines = handle.read().splitlines()

    table = "(还没进任何表 → [dependencies])"
    in_target_table = False
    table_line = 0
    bad = []

    for number, line in enumerate(lines, 1):
        header = re.match(r"^\[([^\]]+)\]", line)
        if header:
            table = header.group(1)
            table_line = number
            # 只管 `[target.*.dependencies]` 这一种表 —— `package` / `lib` /
            # `profile.*` 这些表里出现 `名字 = 值` 是天经地义的，不是错。
            in_target_table = table.startswith("target.")
            continue

        entry = re.match(r"^([A-Za-z0-9_-]+)\s*=", line)
        if not entry:
            continue
        name = entry.group(1)

        # 不在 target 表里（也就是在 `[dependencies]` 里）→ 放行。
        # 在 target 表里但属于 ALLOWED（确实平台专有）→ 放行。
        if not in_target_table or name in ALLOWED:
            continue

        bad.append((number, name, table, table_line))

    if not bad:
        print("✓ 没有通用依赖被误放进 [target.*] 段")
        return 0

    print(f"✗ 有 {len(bad)} 个通用依赖落在了平台段里（只对该平台生效）:\n")
    for number, name, table, table_line in bad:
        print(f"  第 {number} 行: {name}")
        print(f"      却被算进了第 {table_line} 行开始的: [{table}]")
    print(
        "\n  这些依赖若本该对所有平台生效，请把它们移回 [dependencies] ——\n"
        "  TOML 里表头之后的一切都归那一张表。"
    )
    return 1


if __name__ == "__main__":
    sys.exit(main())