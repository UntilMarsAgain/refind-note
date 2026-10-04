#!/usr/bin/env bash
#
# 把「自己的改动」重新贴回 `tauri android init` 生成的工程。
#
# ## 为什么需要这个脚本
#
# `src-tauri/gen/android/` 整个是 **`tauri android init` 的生成物** —— 重新 init 会
# 把它整个重建，我们写在里面的东西**不留**。但有两处是我们自己的、必须活着：
#
# 1. `MainActivity.kt`：沉浸式（隐藏系统栏）。这是**体验**的一部分 ——
#    少了它，屏幕上那三颗导航键就一直在，而正文被压掉一整条。
# 2. `app/build.gradle.kts` 里的 `signingConfigs.debugSign`：debug 签名。
#    没有它 release 构建出的是 unsigned APK，装不上真机。
#
# ## 什么时候需要跑
#
# - 刚跑过 `pnpm tauri android init`
# - `gen/android` 被删过（它不入库之外的构建产物常被顺手清掉）
# - 换了 Tauri 版本之后（生成物变了，想确认自己的改动还在）
#
# ## 幂等
#
# 可以重复跑：`MainActivity.kt` 是整份覆盖，`build.gradle.kts` 的签名段先查后加。
#
# 用法：
#     ./android-overlay/apply.sh

set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
gen="$root/src-tauri/gen/android"
here="$root/android-overlay"

activity="$gen/app/src/main/java/com/untilmarsagain/refindnote/MainActivity.kt"
gradle="$gen/app/build.gradle.kts"

if [[ ! -d "$gen" ]]; then
    echo "找不到 $gen —— 先跑 'pnpm tauri android init'。" >&2
    exit 1
fi

# ---- 1. 沉浸式主界面 ----
mkdir -p "$(dirname "$activity")"
cp "$here/MainActivity.kt" "$activity"
echo "已贴回 MainActivity.kt（沉浸式：隐藏系统栏）"

# ---- 2. debug 签名 ----
if ! grep -q "debugSign" "$gradle"; then
    python3 - "$gradle" <<'PY'
import sys, pathlib

path = pathlib.Path(sys.argv[1])
text = path.read_text()

signing_configs = '''signingConfigs {
  // **仅供本机自测**。密钥路径来自环境变量 `REFIND_DEBUG_KEYSTORE`（见 `~/.zshenv`），
  // 所以别人克隆这个仓库不会被卷进我的签名里。
  create("debugSign") {
    val storePath = System.getenv("REFIND_DEBUG_KEYSTORE")
        ?: (project.findProperty("refindDebugKeystore") as String?)
    if (storePath != null && file(storePath).exists()) {
      storeFile = file(storePath)
      storePassword = "android"
      keyAlias = "androiddebugkey"
      keyPassword = "android"
    }
  }
}

'''

anchor = "    signingConfigs {"
# 在 android { } 里找 buildTypes 之前的位置插入
marker = "    buildTypes {"
if marker not in text:
    raise SystemExit("build.gradle.kts 里找不到 buildTypes —— 生成物结构变了，手动贴一下")

text = text.replace(marker, signing_configs + marker, 1)

wire = '''      // 有密钥才签；没有就留空 —— 那样产出的 APK 是 unsigned，装不上真机。
      // 明确失败比装到一半才报错好。
      val debugSign = signingConfigs.findByName("debugSign")
      if (debugSign != null && debugSign.storeFile?.exists() == true) {
        signingConfig = debugSign
      } else {
        logger.warn("没有找到 debug keystore，release APK 将是未签名的（装不上真机）。")
      }
'''

release = '      getByName("release") {'
if release not in text:
    raise SystemExit("build.gradle.kts 里找不到 release buildType")
text = text.replace(release, release + "\n" + wire, 1)

path.write_text(text)
print("已贴回 build.gradle.kts 的 debug 签名配置")
PY
else
    echo "build.gradle.kts 已有 debugSign，跳过"
fi

# ---- 3. 图标 ----
#
# `tauri icon` 把图标生成到 `src-tauri/icons/android/`，而打包读的是
# `src-tauri/gen/android/app/src/main/res/` —— 后者是 init 生成的，带着**当时**那份，
# 所以图标改了不会自动跟着变。这一步不做的话，装出来还是旧图标
# （而桌面看起来"没生效"，容易误判成设计问题）。
if [[ -d "$root/src-tauri/icons/android" ]]; then
    res="$gen/app/src/main/res"
    for dir in "$root"/src-tauri/icons/android/mipmap-*dpi; do
        [[ -d "$dir" ]] || continue
        name="$(basename "$dir")"
        mkdir -p "$res/$name"
        cp "$dir"/*.png "$res/$name/"
    done
    # 自适应图标的入口 xml 与它引用的背景色
    cp "$root/src-tauri/icons/android/mipmap-anydpi-v26/ic_launcher.xml" \
       "$res/mipmap-anydpi-v26/" 2>/dev/null || {
        mkdir -p "$res/mipmap-anydpi-v26"
        cp "$root/src-tauri/icons/android/mipmap-anydpi-v26/ic_launcher.xml" \
           "$res/mipmap-anydpi-v26/"
    }
    cp "$root/src-tauri/icons/android/values/ic_launcher_background.xml" "$res/values/"
    echo "已同步 Android 图标（含自适应图标入口与背景色）"
else
    echo "找不到 src-tauri/icons/android/ —— 先跑 'npx tauri icon app-icon.svg' 与 'app-icon-android.svg'"
fi

echo
echo "完成。"
