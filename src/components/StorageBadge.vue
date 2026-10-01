<script setup lang="ts">
import { computed } from "vue";
import type { Protection } from "../bindings/note.ts";

/**
 * 这一页在磁盘上是怎么存的。
 *
 * 值全部来自 blob 的**明文头**，所以不解锁就能显示 —— "这一页是加密的"这件事
 * 不该等人输了口令才知道。
 *
 * 每一层各给一枚徽章，从内到外排开；带 gpg 加密或口令时标出"封"的样子：
 * 扫一眼就知道这页的字节不是明文。
 */
const props = defineProps<{
  protection: Protection;
}>();

interface Layer {
  key: string;
  label: string;
  /** 这一层让字节不再是明文 */
  sealed: boolean;
}

const layers = computed<Layer[]>(() => {
  const protection = props.protection;
  const found: Layer[] = [];

  if (protection.compress) {
    found.push({ key: "compress", label: "压缩", sealed: false });
  }
  if (protection.sign) {
    // 把密钥标识写出来：只说"有签名"等于没说，得知道是谁签的
    found.push({ key: "sign", label: `GPG 签名 ${protection.sign}`, sealed: false });
  }
  if (protection.encrypt) {
    found.push({ key: "encrypt", label: `GPG 加密 ${protection.encrypt}`, sealed: true });
  }
  if (protection.symmetric) {
    found.push({ key: "symmetric", label: "口令加密", sealed: true });
  }

  return found;
});

/** 什么都没做就是"原样" */
const label = computed(() =>
  layers.value.length > 0 ? layers.value.map((layer) => layer.label).join(" · ") : "原样",
);
</script>

<template>
  <span class="storage" :title="`落盘封装：${label}`">
    <span v-if="layers.length === 0" class="storage__badge">原样</span>
    <span
      v-for="layer in layers"
      :key="layer.key"
      class="storage__badge"
      :class="{ 'storage__badge--sealed': layer.sealed }"
    >
      {{ layer.label }}
    </span>
  </span>
</template>

<style scoped>
.storage {
  display: inline-flex;
  flex-wrap: wrap;
  gap: 4px;
  align-items: center;
}

.storage__badge {
  display: inline-flex;
  align-items: center;
  padding: 1px 7px;
  border: 1px solid var(--border);
  border-radius: 999px;
  color: var(--text-dim);
  font-size: 11.5px;
  letter-spacing: 0.02em;
  white-space: nowrap;
}

/* 加过密的用主题色描边：不是错误状态，只是"这一份不是明文" */
.storage__badge--sealed {
  border-color: var(--accent-soft);
  color: var(--accent-soft);
}
</style>
