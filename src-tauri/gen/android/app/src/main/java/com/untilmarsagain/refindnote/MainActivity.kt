package com.untilmarsagain.refindnote

import android.os.Bundle
import androidx.activity.enableEdgeToEdge
import androidx.core.view.WindowCompat
import androidx.core.view.WindowInsetsCompat
import androidx.core.view.WindowInsetsControllerCompat

/**
 * 主界面。
 *
 * ## 为什么多这一段：让系统栏**藏起来**，而不是让界面绕开它
 *
 * 原先靠 CSS 的 `env(safe-area-inset-bottom)` 把底部按钮抬到导航栏之上
 * （实测那个值是 48px）。但那只是"绕开"：屏幕上仍然**画着**那三颗导航键，
 * 而它们占掉的是一整条 —— 一个记事类应用，正文被压掉一条总归不如意。
 *
 * 所以改成 **沉浸式（immersive）**：系统栏隐藏，正文铺满整块屏幕。
 *
 * ## 用户怎么把系统栏叫回来
 *
 * **从屏幕边缘往里滑**。这是 Android 既定的交互（`BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE`
 * 就是干这个的），临时显示几秒后自动再藏。任何"藏了但用户找不回来"的做法都是错的：
 * 用户被关在自己的应用里，连返回键都按不到。
 *
 * 另外本程序有**自己的**返回（顶栏那颗后退）与标签栏，手指够得到。
 *
 * ## 为什么用 `WindowInsetsControllerCompat` 而不是老 API
 *
 * 后者在 API 30 起废弃，而 `minSdk = 24`，所以走 AndroidX 那套 ——
 * 一份代码覆盖全部版本，`systemBars()` 的行为差异由它处理。
 */
class MainActivity : TauriActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        enableEdgeToEdge()
        super.onCreate(savedInstanceState)

        // 切进沉浸式，并声明"上滑临时显示"的行为
        WindowCompat.setDecorFitsSystemWindows(window, false)
        WindowInsetsControllerCompat(window, window.decorView).apply {
            systemBarsBehavior =
                WindowInsetsControllerCompat.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE
            hide(WindowInsetsCompat.Type.systemBars())
        }
    }

    /**
     * 窗口重新可见时（从后台切回来）**重新藏一次**。
     *
     * 少了这一步：从别的应用切回来时系统会把导航键还回来并一直留着，
     * 于是底部那条又压回来了 —— 而用户没有做任何事，不该由他去重新藏。
     */
    override fun onResume() {
        super.onResume()
        WindowInsetsControllerCompat(window, window.decorView).apply {
            systemBarsBehavior =
                WindowInsetsControllerCompat.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE
            hide(WindowInsetsCompat.Type.systemBars())
        }
    }
}