#!/usr/bin/env python3
"""
生成第三方许可证声明（Markdown 格式），带版本号并自动合并重复包。

用法:
    python gen_licenses.py

依赖:
    - Node.js (npx 会自动拉取 license-checker)
    - cargo-license (cargo install cargo-license)

输出:
    THIRD_PARTY_NOTICES.md
"""

import json
import subprocess
import sys
from collections import defaultdict
from pathlib import Path

ROOT = Path(__file__).resolve().parent
FRONTEND_DIR = ROOT            # 前端 package.json 所在目录
RUST_DIR = ROOT / "src-tauri"  # Rust Cargo.toml 所在目录
OUTPUT = ROOT / "THIRD_PARTY_NOTICES.md"

# 你自己的项目名，用来排除掉
OWN_PACKAGE_NAME = "refind-note"


def run(cmd, cwd):
    """跑一条命令，返回 stdout。失败时打印 stderr 并退出。"""
    print(f"$ {' '.join(cmd)}  (cwd={cwd})", file=sys.stderr)
    result = subprocess.run(
        cmd, cwd=cwd, capture_output=True, text=True, shell=False
    )
    if result.returncode != 0:
        print(result.stderr, file=sys.stderr)
        sys.exit(f"命令失败: {' '.join(cmd)}")
    return result.stdout


# ---------- 前端 ----------

def scan_frontend():
    """用 license-checker 扫描 package.json 的生产依赖。返回 list[dict]。"""
    raw = run(
        [
            "npx", "--yes", "license-checker",
            "--production",              # 只扫 dependencies，排除 devDependencies
            "--json",
            "--excludePrivatePackages",  # 排除 private:true 的包（包括你自己）
        ],
        cwd=FRONTEND_DIR,
    )
    data = json.loads(raw)

    packages = []
    for key, info in data.items():
        # key 形如 "vue@3.5.13"
        name, _, version = key.rpartition("@")
        if not name:
            name, version = key, ""

        if name == OWN_PACKAGE_NAME:
            continue

        packages.append({
            "name": name,
            "version": version,
            "license": info.get("licenses", "UNKNOWN"),
            "repository": _normalize_repo(info.get("repository")),
            "copyright": _extract_copyright(info.get("copyright")),
        })
    return packages


def _normalize_repo(repo):
    """license-checker 的 repository 可能是 str 或 dict，统一成 URL。"""
    if not repo:
        return ""
    if isinstance(repo, dict):
        return repo.get("url", "")
    return str(repo)


def _extract_copyright(c):
    """license-checker 的 copyright 字段有时是 str，有时是 list。"""
    if not c:
        return ""
    if isinstance(c, list):
        return " / ".join(str(x) for x in c)
    return str(c)


# ---------- Rust ----------

def scan_rust():
    """用 cargo-license 扫描 Cargo.toml 的生产依赖。返回 list[dict]。"""
    raw = run(
        ["cargo", "license", "--json", "--avoid-dev-deps"],
        cwd=RUST_DIR,
    )
    data = json.loads(raw)

    packages = []
    for pkg in data:
        name = pkg.get("name", "")
        if name == OWN_PACKAGE_NAME:
            continue
        packages.append({
            "name": name,
            "version": pkg.get("version", ""),
            "license": pkg.get("license", "UNKNOWN"),
            "repository": pkg.get("repository", "") or "",
            "copyright": _extract_copyright(pkg.get("authors")),
        })
    return packages


# ---------- 合并重复包 ----------

def merge_packages(packages):
    """
    把同名包合并成一条：
      - versions: 排序去重后的版本列表
      - repository / copyright: 取第一个非空值
      - licenses: 收集所有出现过的 license
    """
    merged = {}
    for pkg in packages:
        name = pkg["name"]
        if name not in merged:
            merged[name] = {
                "name": name,
                "versions": set(),
                "licenses": set(),
                "repository": "",
                "copyright": "",
            }
        entry = merged[name]
        if pkg["version"]:
            entry["versions"].add(pkg["version"])
        if pkg["license"]:
            entry["licenses"].add(pkg["license"])
        if not entry["repository"] and pkg["repository"]:
            entry["repository"] = pkg["repository"]
        if not entry["copyright"] and pkg["copyright"]:
            entry["copyright"] = pkg["copyright"]

    result = []
    for entry in merged.values():
        versions = sorted(entry["versions"], key=_version_key)
        result.append({
            "name": entry["name"],
            "version": ", ".join(versions) if versions else "",
            "license": ", ".join(sorted(entry["licenses"])) or "UNKNOWN",
            "repository": entry["repository"],
            "copyright": entry["copyright"],
        })
    return result


def _version_key(v):
    """把版本号转成可排序的元组，'1.2.10' 排在 '1.2.9' 后面。"""
    parts = []
    for seg in v.split("."):
        if seg.isdigit():
            parts.append((0, int(seg)))
        else:
            parts.append((1, seg))
    return parts


# ---------- 输出 ----------

def render_markdown(packages):
    lines = ["（由程序扫描，自动生成）", ""]
    lines.append("")

    for pkg in sorted(packages, key=lambda p: p["name"].lower()):
        name = pkg["name"]
        version = pkg["version"]
        repo = pkg["repository"]
        copyright_text = pkg["copyright"] or f"(c) {name} contributors"

        label = f"{name}@{version}" if version else name
        if repo:
            lines.append(f"- [{label}]({repo}): {copyright_text}")
        else:
            lines.append(f"- {label}: {copyright_text}")

    lines.append("")
    return "\n".join(lines)


def main():
    print("扫描前端依赖（仅生产依赖）...", file=sys.stderr)
    frontend = scan_frontend()

    print("扫描 Rust 依赖（仅生产依赖）...", file=sys.stderr)
    rust = scan_rust()

    all_pkgs = frontend + rust
    print(f"合并前 {len(all_pkgs)} 条记录", file=sys.stderr)

    merged = merge_packages(all_pkgs)
    print(f"合并后 {len(merged)} 个唯一包", file=sys.stderr)

    md = render_markdown(merged)
    OUTPUT.write_text(md, encoding="utf-8")
    print(f"已写入 {OUTPUT}", file=sys.stderr)


if __name__ == "__main__":
    main()