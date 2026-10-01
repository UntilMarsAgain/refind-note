<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from "vue";
import { invoke } from "@tauri-apps/api/core";
import type { Protection, ProtectionReport } from "../../bindings/note.ts";
import { flash } from "../../core/notice.ts";
import { forgetPassphrase } from "../../core/preferences.ts";

/**
 * 这一页在磁盘上是怎么存的。
 *
 * 值全部来自 blob 的**明文头**，所以不解锁就能显示 —— "这一页是加密的"这件事
 * 不该等人输了口令才知道。
 *
 * 徽章只说"有什么层"（已压缩 / 已签名 / 已加密 / 口令加密），一句话一枚；
 * 细节（谁签的、验过没有、本机解不解得开）**点开才问后端** —— 那是要花时间的活，
 * 不该在列一张历史清单时替每一行都做一遍。
 */
const props = defineProps<{
  protection: Protection;
  /** 哪一篇：给了才问得动后端那些细节 */
  title?: string;
  /** 哪一版（地址里的版本 token）；不给或 null = 最新版 */
  reference?: string | null;
}>();

interface Layer {
  key: "compress" | "sign" | "encrypt" | "symmetric";
  label: string;
  /** 这一层让字节不再是明文 */
  sealed: boolean;
  /** 点开有细节可看（压缩没有） */
  reportable: boolean;
}

const layers = computed<Layer[]>(() => {
  const protection = props.protection;
  const found: Layer[] = [];

  if (protection.compress) {
    found.push({ key: "compress", label: "已压缩", sealed: false, reportable: false });
  }
  if (protection.sign) {
    found.push({ key: "sign", label: "已签名", sealed: false, reportable: true });
  }
  if (protection.encrypt) {
    found.push({ key: "encrypt", label: "已加密", sealed: true, reportable: true });
  }
  if (protection.symmetric) {
    found.push({ key: "symmetric", label: "口令加密", sealed: true, reportable: true });
  }

  return found;
});

/** 什么都没做就是"原样" */
const label = computed(() =>
  layers.value.length > 0 ? layers.value.map((layer) => layer.label).join(" · ") : "原样",
);

/** 哪一枚徽章点开了；一次只开一个 */
const openedKey = ref<string | null>(null);

/** 封装细节。点开才去问，问过一次就留着 —— 同一版的事实不会变 */
const report = ref<ProtectionReport | null>(null);
const problem = ref("");
const asking = ref(false);

async function ask() {
  if (report.value || asking.value || !props.title) {
    return;
  }
  asking.value = true;
  problem.value = "";
  try {
    report.value = await invoke<ProtectionReport>("protection_report", {
      title: props.title,
      reference: props.reference ?? null,
    });
  } catch (error) {
    problem.value = String(error);
  } finally {
    asking.value = false;
  }
}

function toggle(key: string) {
  openedKey.value = openedKey.value === key ? null : key;
  if (openedKey.value) {
    void ask();
  }
}

/** 这一版的口令正攥在这次会话里（只有它忘得掉） */
const holdingPassphrase = computed(
  () => openedKey.value === "symmetric" && report.value?.passphrase_ready === true,
);

/**
 * 忘掉这一篇的口令。
 *
 * 口令只在这次会话的内存里，"忘掉"就是把它丢掉 —— 下次读这一篇要重新输入。
 * 给"口令已经在别处记下了、不想让它一直留在内存里"的人用。
 */
async function forget() {
  if (!props.title) {
    return;
  }
  try {
    await forgetPassphrase(props.title);
    openedKey.value = null;
    report.value = null;
    flash(`已忘掉「${props.title}」的口令；下次读它要重新输入`);
  } catch (error) {
    problem.value = String(error);
  }
}

/** 换了一篇或换了一版：结论作废，重新问 */
watch(
  () => `${props.title ?? ""}|${props.reference ?? ""}`,
  () => {
    report.value = null;
    problem.value = "";
  },
);

/** 弹窗里那几行，按点开的是哪一层来 */
const rows = computed<{ label: string; value: string }[]>(() => {
  if (asking.value) {
    return [{ label: "正在查", value: "……" }];
  }
  if (problem.value) {
    return [{ label: "查不动", value: problem.value }];
  }

  const found = report.value;
  if (!found) {
    return [];
  }

  switch (openedKey.value) {
    case "sign": {
      if (found.signature) {
        return [
          { label: "结果", value: found.signature.verified ? "通过" : "没通过" },
          { label: "信任", value: found.signature.trust ?? "（没查到）" },
          { label: "签名者", value: found.signature.key },
          { label: "说明", value: found.signature.detail },
        ];
      }
      return [
        { label: "结果", value: "验不了" },
        { label: "原因", value: found.signature_problem ?? "这一版没有签名层" },
      ];
    }
    case "encrypt": {
      if (!found.encryption) {
        return [{ label: "结果", value: "这一版没有 gpg 加密层" }];
      }
      return [
        { label: "加密到", value: found.encryption.key },
        { label: "本机", value: found.encryption.detail },
      ];
    }
    case "symmetric":
      return [
        {
          label: "这次会话",
          value:
            found.passphrase_ready === true
              ? "已经有这一版的口令：读得动"
              : "还没输过这一版的口令：读之前要先解锁",
        },
      ];
    default:
      return [];
  }
});

/** 点到别处就收起：弹窗是看细节用的，不该占着屏幕 */
const rootEl = ref<HTMLElement | null>(null);

function onDocumentClick(event: MouseEvent) {
  const root = rootEl.value;
  if (root && !root.contains(event.target as Node)) {
    openedKey.value = null;
  }
}

function onKeydown(event: KeyboardEvent) {
  if (event.key === "Escape") {
    openedKey.value = null;
  }
}

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
      <!-- 用 span 装的按钮：这些徽章会出现在**历史列表的行按钮里面**，
           里面再套一个 <button> 是非法结构，点击也会一并触发行本身 -->
      <span
          v-if="layer.reportable && title"
          class="storage__badge storage__badge--ask"
          role="button"
          tabindex="0"
          :aria-expanded="openedKey === layer.key"
          title="点开看这一层的细节"
          @click.stop="toggle(layer.key)"
          @keydown.enter.stop.prevent="toggle(layer.key)"
          @keydown.space.stop.prevent="toggle(layer.key)"
      >
        {{ layer.label }}
      </span>

      <span
          v-else
          class="storage__badge"
          :class="{ 'storage__badge--sealed': layer.sealed }"
      >
        {{ layer.label }}
      </span>
    </template>

    <!-- 点开才展开的细节。它挂在这行徽章底下，不挡正文 -->
    <span v-if="openedKey" class="report" @click.stop>
      <span v-for="row in rows" :key="row.label" class="report__row">
        <span class="report__label">{{ row.label }}</span>
        <span class="report__value">{{ row.value }}</span>
      </span>

      <!--
        口令只在这次会话里攥着。"我不想让它一直留着"是一条真实的诉求，
        所以给一个出口 —— gpg 那几层的口令在 gpg-agent 手里（见下面那句），
        程序碰不到，也就忘不掉。
      -->
      <span v-if="holdingPassphrase" class="report__action">
        <button type="button" class="report__btn" @click="forget">
          忘掉这次会话里的口令
        </button>
      </span>
      <span v-if="openedKey === 'encrypt'" class="report__note">
        gpg 的口令由 gpg-agent 保管，这个程序碰不到（要清就
        <code>gpgconf --kill gpg-agent</code>）。
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

/* 点得开的徽章：悬停给一点提示，免得人不知道这里能点 */
.storage__badge--ask {
  cursor: pointer;
}

.storage__badge--ask:hover,
.storage__badge--ask:focus-visible {
  outline: none;
  border-color: var(--accent-soft);
  color: var(--text);
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
  grid-template-columns: minmax(64px, auto) 1fr;
  gap: 10px;
  align-items: baseline;
}

.report__label {
  color: var(--text-dim);
}

.report__action {
  margin-top: 2px;
}

.report__btn {
  padding: 4px 10px;
  border: 1px solid var(--border);
  border-radius: 6px;
  background: transparent;
  color: var(--text-dim);
  font: inherit;
  font-size: 12px;
  cursor: pointer;
}

.report__btn:hover {
  border-color: var(--danger);
  color: var(--danger);
}

.report__note {
  color: var(--text-dim);
  font-size: 11.5px;
  line-height: 1.6;
}

.report__note code {
  padding: 0 4px;
  border-radius: 3px;
  background: var(--hover);
  font-family: var(--mono-font);
}

.report__value {
  color: var(--text);
  overflow-wrap: anywhere;
  /* 这些值是要抄下来贴进别处的 */
  -webkit-user-select: text;
  user-select: text;
}
</style>
