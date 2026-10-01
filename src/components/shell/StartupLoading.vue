<script setup lang="ts">
import { startupNote } from "../../core/startup.ts";

/**
 * 启动还在跑时顶替主区域的那张加载页。
 *
 * 与错误页同一个位置、同一套外形：启动的三种状态在主区域上是三选一，
 * 切换时不会跳一下。
 *
 * 那一行字是**跟着步骤走**的（见 `startup.ts`）：打开目录、读偏好、与云端同步……
 * 同步可能要跑一会儿，所以进度就写在这里，而不是让人对着一个转圈猜。
 */
</script>

<template>
  <div class="loading">
    <div class="loading__box">
      <span class="loading__spinner" aria-hidden="true" />
      <p class="loading__text" role="status">{{ startupNote }}</p>
    </div>
  </div>
</template>

<style scoped>
.loading {
  display: flex;
  flex: 1 1 auto;
  align-items: center;
  justify-content: center;
  min-width: 0;
  padding: 24px;
}

.loading__box {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 16px 22px;
  border: 1px solid var(--border);
  border-radius: 10px;
  background: var(--surface);
}

.loading__spinner {
  width: 16px;
  height: 16px;
  border: 2px solid var(--border);
  /* 一圈亮色缺口，转起来就是进度感 */
  border-top-color: var(--accent);
  border-radius: 50%;
  animation: loading-spin 700ms linear infinite;
}

.loading__text {
  margin: 0;
  color: var(--text-dim);
  font-size: 13.5px;
}

@keyframes loading-spin {
  to {
    transform: rotate(360deg);
  }
}

/* 不喜欢动效的人：换成呼吸，信息（"还在跑"）不能丢 */
@media (prefers-reduced-motion: reduce) {
  .loading__spinner {
    animation: loading-pulse 1.4s ease-in-out infinite;
  }

  @keyframes loading-pulse {
    0%,
    100% {
      opacity: 0.35;
    }

    50% {
      opacity: 1;
    }
  }
}
</style>
