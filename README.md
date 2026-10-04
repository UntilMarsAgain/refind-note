# 编译说明

这份文档只讲**怎么把代码编译出来**。项目本身是什么不写在这儿。

---

## 目录

- [一句话命令](#一句话命令)
- [产物在哪](#产物在哪)
- [桌面（Linux）](#桌面linux)
- [Android](#android)
- [环境变量](#环境变量)
- [国内镜像](#国内镜像)
- [踩过的坑](#踩过的坑)

---

## 一句话命令

```sh
pnpm install                # 装前端依赖（首次）

pnpm tauri build            # 桌面（Linux）
pnpm tauri android build --apk   # Android，装真机用这个
pnpm tauri android build --aab   # Android，上架用这个
```

开发时：

```sh
pnpm tauri dev              # 桌面，热重载
pnpm tauri android dev      # 接到设备/模拟器上
```

---

## 产物在哪

| 是什么 | 路径 |
|---|---|
| 桌面可执行文件 | `target/release/refind-note` |
| 前端产物 | `dist/`（已被打进二进制，不用管） |
| Android 中间产物 | `target/aarch64-linux-android/release/` 等四个 ABI |
| Android 安装包 | `src-tauri/gen/android/app/build/outputs/apk/universal/release/app-universal-release.apk` |
| Android Android Bundle | `src-tauri/gen/android/app/build/outputs/bundle/release/` |

> **`target/` 在仓库根，不在 `src-tauri/` 里。** 根目录有一个 `Cargo.toml`
> （workspace 根），Rust 的产物位置由它决定，与从哪个目录跑 cargo 无关。
> Tauri 项目默认会放在 `src-tauri/target`，这里改过来了 —— 理由写在根
> `Cargo.toml` 的注释里。

---

## 桌面（Linux）

### 依赖

```sh
sudo pacman -S webkit2gtk-4.1 gtk3 gpgme \
                gst-plugins-base gst-plugins-good gst-libav
```

- `webkit2gtk-4.1` / `gtk3` —— webview 本体，缺了编不过
- `gpgme` —— GPG 签名/加密那一层（系统库）
- `gst-*` —— 音视频解码，不装的话网页里嵌的视频放不出来

这份清单与 `packaging/arch/PKGBUILD` 里那份一致（那份已实际编过）。

### 其它前置

- Rust（`rustup`，用 stable）
- Node.js 与 pnpm

Tauri 官方还提到 `libxdo` / `librsvg` / `libayatana-appindicator3`
（托盘与某些 Linux 特性用）。本项目**没有用托盘**，实测这三个都没装、
二进制也没有链接它们，照样编得过 —— 所以不列为必需。

---

## Android

Android 比桌面多几步，因为**工程本身是生成的**，而且**生成之后要贴回我们自己的改动**。

### 一次性准备

```sh
# 1. JDK 17（见坑 4 —— 不要用 Studio 自带的那个）
sudo pacman -S jdk17-openjdk

# 2. Android SDK：Studio 的 SDK Manager 里勾这几项
#      Android SDK Platform
#      Android SDK Platform-Tools
#      NDK (Side by side)  —— 版本见下
#      Android SDK Build-Tools
#      Android SDK Command-line Tools

# 3. Rust 的四个 Android target
rustup target add aarch64-linux-android armv7-linux-androideabi \
                 i686-linux-android x86_64-linux-android

# 4. 生成 Gradle 工程
pnpm tauri android init

# 5. 贴回我们自己的改动（见下）
./android-overlay/apply.sh
```

### 版本要求

| 东西 | 版本 | 怎么定的 |
|---|---|---|
| JDK | **17** | Gradle 插件要 17。Studio 自带的 JBR 是 25，**不能用**（坑 4） |
| NDK | **30.0.16248370** | r23 起不再提供不带 API level 的编译器名，得写死版本 |
| Gradle | 9.6.1 | 由 `gradle-wrapper.properties` 决定，自动下 |
| Build-Tools | 36.0.0 | |
| compileSdk / targetSdk | 37 | 工程里声明 |
| minSdk | 24 | |

### `gen/android` 是生成物

`src-tauri/gen/android/` **整个是 `tauri android init` 生成的**，所以它不在版本库里
（见 `.gitignore`）。重新 init 会重建它，我们写在里面的东西不留。

因此有 `android-overlay/`，负责把三处自己的改动贴回去：

```sh
./android-overlay/apply.sh
```

它做三件事，**每次 init 之后都要跑**：

1. **沉浸式**（`MainActivity.kt`）—— 隐藏系统栏。少了它，屏幕上那三颗导航键
   一直在，正文被压掉一整条
2. **debug 签名**（`build.gradle.kts`）—— 少了它 release 构建出的是 unsigned
   APK，装不上真机
3. **同步 Android 图标** —— `tauri icon` 把图标生成到 `src-tauri/icons/android/`，
   而打包读的是 `gen/android/app/src/main/res/`。少了这一步，装出来还是旧图标，
   而那看起来像"设计没生效"，容易误判

脚本是幂等的，重复跑不会重复插入。

### 改图标

```sh
npx tauri icon app-icon.svg            # 桌面那套
npx tauri icon app-icon-android.svg    # Android 那套 ← 必须最后跑
./android-overlay/apply.sh              # 把结果同步进 gen/
```

**顺序不能反。** 两条 `tauri icon` 共用同一个输出目录，后跑的会覆盖前一条的
`android/` 部分 —— Android 版不最后跑，启动器拿到的就是桌面那版。

### 签名

`apply.sh` 加的 `signingConfigs.debugSign` 从环境变量 `REFIND_DEBUG_KEYSTORE`
读密钥路径。**没有密钥就编出 unsigned 的 APK，装不上真机**，而 Gradle 只会打一条
警告 —— 所以装之前看一眼构建日志。

生成一把自测用的（口令固定 `android`）：

```sh
keytool -genkeypair -v -keystore ~/.android/debug.keystore \
        -storepass android -keypass android \
        -alias androiddebugkey -keyalg RSA -keysize 2048 -validity 10000 \
        -dname "CN=Android Debug,O=Android,C=US"
export REFIND_DEBUG_KEYSTORE="$HOME/.android/debug.keystore"
```

⚠️ 这把钥匙的生成命令是公开的、口令是固定的，**只能用于本机自测，不能拿去发布**。
发布签名要另配并妥善保管，那时把 `REFIND_DEBUG_KEYSTORE` 指向那把钥匙即可。

---

## 环境变量

全部写在 `~/.zshenv`（**不是** `~/.zshrc` —— 后者只对交互式 shell 生效，
而 Tauri 可能是从 IDE 的终端、脚本里发起的）。改完要**新开终端**或
`source ~/.zshenv` 才生效。

```sh
export JAVA_HOME="/usr/lib/jvm/java-17-openjdk"
export ANDROID_HOME="$HOME/Android/Sdk"
export NDK_HOME="$ANDROID_HOME/ndk/30.0.16248370"

# 交叉编译器要能在 PATH 里找到：cargo 是按名字去 PATH 找的，
# 找的是 `aarch64-linux-android21-clang` 这种带 API level 的名字
export PATH="$NDK_HOME/toolchains/llvm/prebuilt/linux-x86_64/bin:$PATH"

export REFIND_DEBUG_KEYSTORE="$HOME/.android/debug.keystore"
```

交叉编译器的配置本身在 `src-tauri/.cargo/config.toml`（仓库内，四��� ABI 的
linker/ar）。

> 为什么那份配置放在 `src-tauri/` 而不是仓库根？因为 **cargo 只从「当前工作
> 目录」向上找配置，不从 manifest 所在目录找**。桌面那条路 CWD 是仓库根，
> Android 那条路 CWD 是 `src-tauri/gen/android`（向上正好到 `src-tauri/`）——
> 所以需要交叉编译配置的地方都能找到它。
> 这也是为什么产物位置改用 workspace 而不是 `build.target-dir`：后者要么写两份
> （漏一份就有一半构建悄悄回到旧位置），要么写环境变量（换机器/CI 就失效）。

---

## 国内镜像

> 只影响下载速度，不影响功能。**Android SDK/NDK 本身不需要配镜像** ——
> 实测 `dl.google.com` 在 35–41 MB/s，直接用就行。（常见的那些镜像站
> 反而没同步 Android SDK 仓库，实测全是 404。）

### 已经配好的

| 配在哪 | 管什么 |
|---|---|
| `~/.zshenv` 的 `RUSTUP_DIST_SERVER` / `RUSTUP_UPDATE_ROOT` | rustup 下载工具链（USTC） |
| `~/.cargo/config.toml` | crates.io 依赖（USTC 稀疏索引） |
| `~/.npmrc` | npm / pnpm（npmmirror） |
| `/etc/pacman.d/mirrorlist` | 系统包（USTC） |
| `~/.gradle/init.gradle` | Gradle 的 Maven 依赖（阿里云）+ 下载超时 |
| `gen/android/gradle/wrapper/gradle-wrapper.properties` 的 `distributionUrl` | Gradle 发行版本身（腾讯云） |

### `~/.gradle/init.gradle` 里那段超时是必要的

```groovy
System.setProperty('org.gradle.internal.http.connectionTimeout', '60000')
System.setProperty('org.gradle.internal.http.socketTimeout', '180000')
System.setProperty('org.gradle.internal.repository.max.retries', '2')
```

Gradle 默认 30 秒/30 秒/重试很多次。国内网络下真正的问题不是"太短"，而是
**重试太积极**：一个慢响应被判成失败、换源再来一遍，于是"卡住"和"慢速"看起来
一模一样。给足余量并把重试压到 2 次之后，真的断网会在几分钟内明确失败。

---

## 踩过的坑

### 1. Gradle 会拿"旧 APK"冒充新的

构建日志写着 `Finished 1 APK`，时间戳也是刚刚的，**但里面是旧内容**。
Gradle 的配置缓存判定"输入没变"，直接复用了上一次的产物。

```sh
# 编完核对时间戳
ls -la src-tauri/gen/android/app/build/outputs/apk/universal/release/*.apk
```

怀疑时强制重跑：

```sh
GRADLE_OPTS="-Dorg.gradle.configuration-cache=false" pnpm tauri android build --apk
```

更可靠的验证是解开看内容（`unzip` 后查 `assets/` 或用 `aapt2 dump resources`）——
时间戳新不代表内容新。

### 2. 改了 `distributionUrl` 会触发一次完整重下

Gradle 用 URL 的哈希当目录名，**URL 一变就指向另一个目录**，那里没有缓存就重新下
130 MB。官方源在国内只有约 28 KB/s（会 307 跳到 GitHub 的资产地址），
表现出来是"卡在一个很奇怪的目录上"。

所以改这个 URL 之前先想清楚。镜像可用性实测：

| 镜像 | 状态 |
|---|---|
| 腾讯云 `mirrors.cloud.tencent.com/gradle/` | ✅ 200 |
| 南京大学 `mirrors.nju.edu.cn/gradle/` | ✅ 200 |
| 华为云 `mirrors.huaweicloud.com/gradle/` | ✅ 200 |
| 阿里云 `mirrors.aliyun.com/gradle/` | ❌ 404 |
| 中科大 `mirrors.ustc.edu.cn/gradle/` | ❌ 404 |

别照抄别处的配置，**逐个验**。

### 3. `plugin windows not initialized`

移动端没有窗口概念，而 `currentWindow()` 返回的对象**方法**会走到后端去取窗口状态，
于是报这个。判据要用后端给的 `platform_kind`（`isMobile()`），
**不能**用"拿到对象没有" —— 后者在移动端照样给得出对象。

### 4. 不要用 Android Studio 自带的 JDK

Studio 自带的 JBR 是 **JDK 25**，而 Gradle 插件要 **17**。用 25 会撞
`Unsupported class file major version`。

### 5. `compileSdk 37` 对应的平台目录名是 `android-37.0`

不是 `android-37`。Gradle 能解析到，但装 SDK 时如果只勾了 `Android 37` 而目录叫
`android-37`，会找不到。装完确认一下：

```sh
ls ~/Android/Sdk/platforms/          # 应有 android-37.0
ls ~/Android/Sdk/platforms/*/android.jar
```

### 6. `tauri icon` 会把启动器背景色覆盖成空模板

`npx tauri icon` 生成的 `values/ic_launcher_background.xml` 是**空的**
（`<resources></resources>`），颜色留给 Android Studio 去填。没有颜色时自适应图标
的底是透明的，浅色壁纸上那道白图形直接看不见。

所以这个文件由 `android-overlay/` 维护（手写），别指望 `tauri icon` 给你填。

### 7. `tauri android init` 会把手写的 Gradle 配置清掉

`gen/android/` 里那个 `signingConfigs.debugSign` 是我们加的，重新 init 之后就没了。
跑完 init 记得 `./android-overlay/apply.sh`。

### 8. 同名颜色定义在两个文件里会直接让构建失败

```text
Execution failed for task ':app:mergeArm64ReleaseResources'
> [color/ic_launcher_background] colors.xml   ← 重复定义
```

这个平时被构建缓存挡着，**只有真的重打包才暴露**。

---

## 附：版本一览

本仓库当前验证过的组合：

| | 版本 |
|---|---|
| Node.js | v26.10.0 |
| pnpm | 11.28.2 |
| Rust | 1.98.1 |
| JDK | 17.0.20.1 |
| Tauri CLI | 2.12.0 |
| Gradle | 9.6.1 |
| NDK | 30.0.16248370 |
| Android Build-Tools | 36.0.0 |
| compileSdk / targetSdk / minSdk | 37 / 37 / 24 |

Rust 各 crate 的确切版本见 `Cargo.lock`。
