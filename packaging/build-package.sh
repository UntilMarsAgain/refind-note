#!/usr/bin/env bash
#
# 打一个可以 `pacman -U` 安装的包。只在本地打包，**不上传 AUR**。
#
# 做的事：核对版本 → 从当前提交导出源码包 → 交给 makepkg。
# 刻意不做的事：不改 PKGBUILD（版本对不上时直接报错，而不是悄悄替你改一行）。
set -euo pipefail

cd "$(dirname "$0")"
here="$(pwd)"

# 版本只有一个来源：tauri.conf.json。PKGBUILD 里那份动不得（上面写着用法），
# 所以这里的职责是**发现不一致**并说清楚，而不是替人做主。
version="$(python3 -c 'import json;print(json.load(open("../src-tauri/tauri.conf.json"))["version"])')"
pkgver="$(sed -n 's/^pkgver=//p' PKGBUILD)"
if [ "$version" != "$pkgver" ]; then
  echo "版本对不上：" >&2
  echo "  src-tauri/tauri.conf.json 里是 $version" >&2
  echo "  packaging/PKGBUILD 里是 $pkgver" >&2
  echo "把 PKGBUILD 的 pkgver 改成 $version 再跑。" >&2
  exit 1
fi

# 工作区必须是干净的：源码包来自 HEAD，脏文件不会进去 ——
# 不说清楚的话，打出来的包和眼前看到的代码可能不是一回事。
if [ -n "$(git status --porcelain)" ]; then
  echo "工作区不干净，先提交（包是从 HEAD 导出的，脏文件不会进包）：" >&2
  git status --short >&2
  exit 1
fi

archive="refind-note-${version}.tar.gz"
echo "==> 导出源码：$archive"
git archive --format=tar.gz --prefix="refind-note-${version}/" -o "$here/$archive" HEAD

echo "==> 生成 .SRCINFO（发布到 AUR 才用得到，这里顺手生成，便于以后要用）"
makepkg --printsrcinfo > .SRCINFO

echo "==> makepkg 开始构建（首次会编译整个 Rust 工程，需要几分钟到十几分钟）"
makepkg --force

echo
echo "==> 完成。可以这样安装："
ls -1 refind-note-*.pkg.tar.zst | sed 's/^/    sudo pacman -U /'
