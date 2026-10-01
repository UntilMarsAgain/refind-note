<script setup lang="ts">
/**
 * 启动还在跑时顶替主区域的那张加载页。
 *
 * 与错误页同一个位置、同一套外形：启动的三种状态在主区域上是三选一，
 * 切换时不会跳一下。
 */
</script>

<template>
  <div class="loading">
    <div class="loading__box">
      <span class="loading__spinner" aria-hidden="true" />
      <p class="loading__text" role="status">正在打开工作目录…</p>
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
