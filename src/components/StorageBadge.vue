<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { Check, ShieldAlert, TriangleAlert } from "@lucide/vue";
import type { Protection, SignatureReport } from "../bindings/note.ts";

/**
 * 这一页在磁盘上是怎么存的。
 *
 * 值全部来自 blob 的**明文头**，所以不解锁就能显示 —— "这一页是加密的"这件事
 * 不该等人输了口令才知道。
 *
 * 每一层各给一枚徽章，从内到外排开；带 gpg 加密或口令时标出"封"的样子：
 * 扫一眼就知道这页的字节不是明文。
 *
 * 签名那一枚还回答"这个签名还算不算数"：给了 `title` 就问后端验一次，
 * 结论（过没过、本机信不信那把公钥）直接写在徽章上，点开才展开细节。
 */
const props = defineProps<{
  protection: Protection;
  /** 哪一篇：给了才问得动后端"这个签名还算不算数" */
  title?: string;
  /** 哪一版（地址里的版本 token）；不给或 null = 最新版 */
  reference?: string | null;
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

/** 有签名层、又知道是哪一篇 —— 这样的徽章才验得了 */
const verifiable = computed(() => props.protection.sign !== null && Boolean(props.title));

/** 验签的结论；`null` = 还没查出来（或压根没有签名层） */
const report = ref<SignatureReport | null>(null);
/** 验签这步自己没跑起来的原因（没有 gpg、这一版的口令没给……） */
const problem = ref("");
const checking = ref(false);
const opened = ref(false);

async function check() {
  if (!verifiable.value) {
    return;
  }
  checking.value = true;
  problem.value = "";
  try {
    report.value = await invoke<SignatureReport | null>("signature_report", {
      title: props.title,
      reference: props.reference ?? null,
    });
  } catch (error) {
    report.value = null;
    problem.value = String(error);
  } finally {
    checking.value = false;
  }
}

/** 换了一篇或换了一版就重新验；签名层没了就没什么可验的 */
watch(
  () => `${props.title ?? ""}|${props.reference ?? ""}|${props.protection.sign ?? ""}`,
  () => {
    report.value = null;
    problem.value = "";
    void check();
  },
  { immediate: true },
);

/** 徽章上那半个字：过没过、信不信 */
const verdict = computed(() => {
  if (checking.value) {
    return "验签中…";
  }
  if (problem.value) {
    return "验不了";
  }
  const found = report.value;
  if (!found) {
    return "没有结论";
  }
  if (!found.verified) {
    return "没通过";
  }
  return found.trust ?? "通过";
});

/** 结论的语气（颜色）：通过、没通过、还没结论 */
const tone = computed(() => {
  if (checking.value || problem.value || !report.value) {
    return "wait";
  }
  return report.value.verified ? "pass" : "fail";
});

/** 弹窗里那几行 —— 与复制出去的是同一份 */
const details = computed(() => {
  const rows: { label: string; value: string }[] = [
    { label: "头里记的签名者", value: props.protection.sign ?? "（没有）" },
  ];
  if (report.value) {
    rows.push({ label: "结果", value: report.value.verified ? "通过" : "没通过" });
    rows.push({ label: "信任", value: report.value.trust ?? "（验签没跑到给出信任的那一步）" });
    rows.push({ label: "说明", value: report.value.detail });
  }
  if (problem.value) {
    rows.push({ label: "没法验", value: problem.value });
  }
  if (checking.value) {
    rows.push({ label: "结果", value: "正在验…" });
  }
  return rows;
});

function toggle() {
  opened.value = !opened.value;
}

/** 点到别处就收起：弹窗是看细节用的，不该占着屏幕 */
function onDocumentClick(event: MouseEvent) {
  const root = rootEl.value;
  if (root && !root.contains(event.target as Node)) {
    opened.value = false;
  }
}

function onKeydown(event: KeyboardEvent) {
  if (event.key === "Escape") {
    opened.value = false;
  }
}

const rootEl = ref<HTMLElement | null>(null);

onMounted(() => {
  document.addEventListener("click", onDocumentClick);
  document.addEventListener("keydown", onKeydown);
});

onBeforeUnmount(() => {
  document.removeEventListener("click", onDocumentClick);
  document.removeEventListener("keydown", onKeydown);
});
</script>

<template>
  <span ref="rootEl" class="storage" :title="`落盘封装：${label}`">
    <span v-if="layers.length === 0" class="storage__badge">原样</span>

    <template v-for="layer in layers" :key="layer.key">
      <!-- 用 span 装的按钮：这枚徽章会出现在**历史列表的行按钮里面**，
           里面再套一个 <button> 是非法结构，点击也会一并触发行本身 -->
      <span
          v-if="layer.key === 'sign' && verifiable"
          class="storage__badge storage__badge--check"
          role="button"
          tabindex="0"
          :aria-expanded="opened"
          title="点开看这次验签的细节"
          @click.stop="toggle"
          @keydown.enter.stop.prevent="toggle"
          @keydown.space.stop.prevent="toggle"
      >
        {{ layer.label }}
        <span class="storage__verdict" :class="`storage__verdict--${tone}`">
          <Check v-if="tone === 'pass'" :size="11" :stroke-width="2.6"/>
          <ShieldAlert v-else-if="tone === 'fail'" :size="11" :stroke-width="2.2"/>
          <TriangleAlert v-else :size="11" :stroke-width="2.2"/>
          {{ verdict }}
        </span>
      </span>

      <span
          v-else
          class="storage__badge"
          :class="{ 'storage__badge--sealed': layer.sealed }"
      >
        {{ layer.label }}
      </span>
    </template>

    <!-- 点开才展开的细节。它挂在徽章底下，不挡正文 -->
    <span v-if="opened" class="report" @click.stop>
      <span v-for="row in details" :key="row.label" class="report__row">
        <span class="report__label">{{ row.label }}</span>
        <span class="report__value">{{ row.value }}</span>
      </span>
    </span>
  </span>
</template>

<style scoped>
.storage {
  position: relative;
  display: inline-flex;
  flex-wrap: wrap;
  gap: 4px;
  align-items: center;
}

.storage__badge {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  padding: 1px 7px;
  border: 1px solid var(--border);
  border-radius: 999px;
  background: transparent;
  color: var(--text-dim);
  font: inherit;
  font-size: 11.5px;
  letter-spacing: 0.02em;
  white-space: nowrap;
}

/* 加过密的用主题色描边：不是错误状态，只是"这一份不是明文" */
.storage__badge--sealed {
  border-color: var(--accent-soft);
  color: var(--accent-soft);
}

/* 验得了的签名徽章是可点的：点开看细节 */
.storage__badge--check {
  cursor: pointer;
}

.storage__badge--check:focus-visible {
  outline: none;
  border-color: var(--accent-soft);
  color: var(--text);
}

.storage__badge--check:hover {
  border-color: var(--accent-soft);
  color: var(--text);
}

.storage__verdict {
  display: inline-flex;
  align-items: center;
  gap: 3px;
}

.storage__verdict--pass {
  color: var(--link-green);
}

.storage__verdict--fail {
  color: var(--link-missing);
}

.storage__verdict--wait {
  color: var(--text-dim);
}

/* 细节卡：贴在这行徽章底下 */
.report {
  position: absolute;
  top: calc(100% + 6px);
  left: 0;
  z-index: 20;
  display: flex;
  flex-direction: column;
  gap: 6px;
  width: max-content;
  max-width: min(420px, 70vw);
  padding: 10px 12px;
  border: 1px solid var(--border);
  border-radius: 8px;
  /* 不透明：它压在正文上，半透明会让底下的字透上来 */
  background: var(--surface);
  box-shadow: 0 10px 28px rgb(0 0 0 / 30%);
  font-size: 12.5px;
  letter-spacing: normal;
  white-space: normal;
}

.report__row {
  display: grid;
  grid-template-columns: minmax(88px, auto) 1fr;
  gap: 10px;
  align-items: baseline;
}

.report__label {
  color: var(--text-dim);
}

.report__value {
  color: var(--text);
  overflow-wrap: anywhere;
  /* 这些值是要抄下来贴进别处的 */
  -webkit-user-select: text;
  user-select: text;
}
</style>
