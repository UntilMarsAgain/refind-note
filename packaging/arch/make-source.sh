#!/usr/bin/env bash
# 把仓库根目录打成 makepkg 要的源码包（放在本目录，名字与 PKGBUILD 里的 source 对上）。
#
#     ./make-source.sh          # 打出 refind-note-<版本>.tar.gz
#     makepkg -si               # 接着构建并安装
#
# 排除的都是"本机生成的、或者能再装回来的"东西：构建产物、前端依赖、仓库历史。
# 注意 pnpm 的 node_modules 里可能有软链，tar 会如实处理（构建时反正要重新 install）。
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$(cd "$here/../.." && pwd)"

version="$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["version"])' "$root/src-tauri/tauri.conf.json")"
name="refind-note-$version"
tarball="$here/$name.tar.gz"

# 版本号在这一带的四处要对得上（tauri.conf.json 是源头，PKGBUILD 是这里要用的那份）：
# 对不上的话打出来的包会顶着旧版本号，装了也不更新（pacman 比的就是它）。
pkgbuild_version="$(sed -n 's/^pkgver=\(.*\)$/\1/p' "$here/PKGBUILD" | head -1)"
if [ "$pkgbuild_version" != "$version" ]; then
    echo "PKGBUILD 里的 pkgver=$pkgbuild_version，配置里是 $version —— 已经改成 $version" >&2
    sed -i "s/^pkgver=.*$/pkgver=$version/" "$here/PKGBUILD"
fi

echo "源码：$root"
echo "版本：$version"

# --transform 把顶层目录名固定成 PKGBUILD 期待的那个（仓库目录叫什么都不影响）
tar -czf "$tarball" \
    --transform "s,^,${name}/," \
    --exclude='./target' \
    --exclude='./src-tauri/target' \
    --exclude='./node_modules' \
    --exclude='./dist' \
    --exclude='./.git' \
    --exclude='./src-tauri/gen' \
    --exclude='./packaging/arch/*.tar.gz' \
    --exclude='./packaging/arch/*.pkg.tar.*' \
    -C "$root" .

echo "打好了：$tarball"
ls -lh "$tarball" | awk '{print $5, $9}'
