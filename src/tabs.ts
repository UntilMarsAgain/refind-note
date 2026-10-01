import { computed, ref } from "vue";
import { resolveAddress, titleOf } from "./address.ts";
import type { ResolvedAddress } from "./bindings/address.ts";

/** 切换标签页的原因 */
export type Movement =
  /** 内部链接、标签栏入口、地址栏回车：都算一次跳转，往历史里推一条 */
  | "push"
  /** 把**脚下这条**历史记录换掉（重定向走它）：不新增记录，后退仍回得来处 */
  | "replace"
  /** 后退 / 前进：地址已在历史里，只挪游标 */
  | "history";

export interface TabState {
  /** 稳定身份：列表动画与拖动重排都靠它，不用下标（下标会随重排变） */
  id: string;
  /** 地址栏里的字。标题栏与它双向绑定；**可能不是规范地址**（编辑中、或刚写错） */
  address: string;
  /** 这个标签页当前在哪：地址**解析并落到仓库上**的结果（`canonical` 就是规范地址）。新标签页是 null */
  route: ResolvedAddress | null;
  /** 标签栏上显示的名字。由 `route` 派生并缓存在这里，便于列表渲染 */
  title: string;
  /** 这个标签页自己的浏览历史。存的是**规范地址**；新标签页是空的 */
  history: string[];
  /** 历史里的位置：后退 / 前进就是挪它。`-1` = 还没去过任何地方 */
  cursor: number;
  /** 上一次停下的滚动位置：切回来时还原 */
  scroll: number;
  /** 上一次地址解析失败的原因（一句给人看的话）。空串表示没有错误 */
  error: string;
}

const NEW_TAB_TITLE = "新标签页";

/**
 * 新标签页落到哪儿。
 *
 * 它本身就是一个地址（`special:newtab`），不是"没有地址" —— 所以地址栏一开就有东西，
 * 后退也回得到一个真实的地方。
 */
const NEW_TAB_ADDRESS = "special:newtab";

/** 关掉的标签页最多记这么多个，够 Ctrl+Shift+T 退回来就行 */
const CLOSED_LIMIT = 24;

/** 标签页 id 的自增序号 */
let sequence = 0;

function nextId(): string {
  sequence += 1;
  return `tab-${sequence}`;
}

/**
 * 创建一个新标签页的**空壳**，与浏览器一致，光标停在地址栏里等输入。
 *
 * 历史是空的、游标是 `-1`：这个标签页还没去过任何地方。
 * 不能拿空串占一条 —— 空串不是地址，能按的后退按不动。
 * 落到新标签页那一页是 [`useTabs::newTab`] 的事：那要问后端，是导航。
 */
export function createTab(): TabState {
  return {
    id: nextId(),
    address: "",
    route: null,
    title: NEW_TAB_TITLE,
    history: [],
    cursor: -1,
    scroll: 0,
    error: "",
  };
}

/**
 * 一扇窗口的全部标签页状态。
 *
 * 刻意做成"调一次、持有一份"的 composable（在 App 里调一次），而不是模块级单例：
 * 状态跟着组件树走，测试里可以起好几份，互不干扰。
 */
export function useTabs() {
  const tabs = ref<TabState[]>([createTab()]);
  const activeIndex = ref(0);
  /** 刚关掉的标签页（Ctrl+Shift+T 用）：后进先出 */
  const closed = ref<TabState[]>([]);
  /**
   * 抖动信号：每次"关掉最后一个标签、于是又新建了一个"就 +1。
   * 只在**这一条路径**上自增 —— 点加号新建、启动时新建都不该抖（那本来就有明确的动作）。
   */
  const shakeTick = ref(0);

  /** 当前标签页。窗口里永远至少有一个标签页，所以实际不会为 null */
  const active = computed<TabState | null>(() => tabs.value[activeIndex.value] ?? null);

  /**
   * 当前标签页的**规范地址**：标题栏失焦 / Esc 回显时以它为准。
   * 解析失败时不写 route，所以它自然还是上一次导航成功的地方。
   */
  const committed = computed(() => active.value?.route?.canonical ?? "");

  /** 后退 / 前进：历史游标决定，跟地址解析没关系 */
  const canGoBack = computed(() => (active.value?.cursor ?? 0) > 0);
  const canGoForward = computed(() => {
    const tab = active.value;
    return tab ? tab.cursor < tab.history.length - 1 : false;
  });

  const canReopen = computed(() => closed.value.length > 0);

  /** 切标签页。**只是挪一下下标** —— 要显示的东西都在那个标签页身上，不必重新解析 */
  function select(index: number): void {
    if (index >= 0 && index < tabs.value.length) {
      activeIndex.value = index;
    }
  }

  async function newTab(): Promise<void> {
    tabs.value.push(createTab());
    activeIndex.value = tabs.value.length - 1;
    await navigate(NEW_TAB_ADDRESS, "push");
  }

  /** 在新标签页里打开某个地址：先建一个空标签页，再让它导航过去 */
  async function newTabWith(address: string): Promise<void> {
    await newTab();
    await navigate(address, "push");
  }

  /** 把关掉的标签页记进"可重开"的栈里，超上限就丢最旧的 */
  function remember(tab: TabState): void {
    closed.value.push(tab);
    if (closed.value.length > CLOSED_LIMIT) {
      closed.value.shift();
    }
  }

  /**
   * 关闭标签页；关掉最后一个就立刻新建一个 —— 空壳窗口没有意义。
   *
   * 新建那一个要抖一下：关闭**是**生效了，只是又开了一个；
   * 不抖的话，点了关闭、界面看着没什么变化，用户会以为没反应。
   */
  async function close(index: number): Promise<void> {
    const closing = tabs.value[index];
    if (closing) {
      remember(closing);
    }

    if (tabs.value.length <= 1) {
      tabs.value = [createTab()];
      activeIndex.value = 0;
      shakeTick.value += 1;
      await navigate(NEW_TAB_ADDRESS, "push");
      return;
    }

    tabs.value.splice(index, 1);
    if (index < activeIndex.value) {
      activeIndex.value -= 1;
    } else if (index === activeIndex.value) {
      // 关掉的是当前这个：落到右边邻居上，没右边了就落到左边
      activeIndex.value = Math.min(index, tabs.value.length - 1);
    }
  }

  /** 关闭其它标签页：只留下点中的那一个（右键菜单里的一项） */
  function closeOthers(index: number): void {
    const keep = tabs.value[index];
    if (!keep) {
      return;
    }
    // 其余的都算"刚关掉"。按从左到右压栈，于是最右边的在最上面 ——
    // 与"一个个关过去"的顺序一致，重新打开时回来的顺序也就一致。
    for (const tab of tabs.value) {
      if (tab !== keep) {
        remember(tab);
      }
    }
    tabs.value = [keep];
    activeIndex.value = 0;
  }

  /** 重新打开最近关掉的那个：插在当前标签页**右边**并切过去（与浏览器一致） */
  function reopenClosed(): void {
    const tab = closed.value.pop();
    if (!tab) {
      return;
    }
    const at = tabs.value.length === 0 ? 0 : activeIndex.value + 1;
    tabs.value.splice(at, 0, tab);
    activeIndex.value = at;
  }

  /** 拖放调整顺序：把 from 位置的挪到 to 位置，并让"当前"仍指向同一个标签页 */
  function move(from: number, to: number): void {
    if (from === to || from < 0 || to < 0 || from >= tabs.value.length || to >= tabs.value.length) {
      return;
    }

    const [moved] = tabs.value.splice(from, 1);
    if (!moved) {
      return;
    }
    tabs.value.splice(to, 0, moved);

    if (activeIndex.value === from) {
      activeIndex.value = to;
    } else if (from < activeIndex.value && to >= activeIndex.value) {
      activeIndex.value -= 1;
    } else if (from > activeIndex.value && to <= activeIndex.value) {
      activeIndex.value += 1;
    }
  }

  /**
   * **唯一的导航入口**：改地址 → 解析 → 把结果落进标签页。
   *
   * 界面上「在哪」的所有变化都必须走这里，不做局部状态拼接。三种情况：
   *
   * - **解析失败**：只记下原因（`error`），**地址栏一个字都不动** ——
   *   用户写错了，应当看见自己输入的内容；
   * - **空地址**：空输入不是地址 —— 保留当前地址，什么都不做；
   * - **成功**：权威副本先落地，然后**回显覆写**（把规范地址写回地址栏）。
   *
   * 返回是否真的落地了一次导航。
   */
  async function navigate(input: string, movement: Movement = "push"): Promise<boolean> {
    const tab = active.value;
    if (!tab) {
      return false;
    }

    tab.error = "";
    let route: ResolvedAddress | null;
    try {
      route = await resolveAddress(input);
    } catch (error) {
      tab.error = String(error);
      return false;
    }

    // 空输入不是地址：什么都不动
    if (route === null) {
      return false;
    }

    // 权威副本先落地，其余（标签名、地址栏）都由它派生
    tab.route = route;
    tab.title = titleOf(route);
    // 回显覆写：规范地址盖掉用户敲的原文
    tab.address = route.canonical;

    if (movement === "push") {
      // 跳转：丢掉原来的"前进"那一截，再压一条。
      // 新标签页的游标是 -1，`slice(0, 0)` 得空数组，正好从零开始。
      tab.history = tab.history.slice(0, tab.cursor + 1);
      tab.history.push(route.canonical);
      tab.cursor = tab.history.length - 1;
    } else if (movement === "replace") {
      // 替换：脚下这条记录改成新地址；长度与游标都不动，
      // 于是"后退"回到的还是进来之前那一页（上锁重定向正是为了这个）
      if (tab.cursor < 0) {
        tab.history = [route.canonical];
        tab.cursor = 0;
      } else {
        tab.history[tab.cursor] = route.canonical;
      }
    }

    return true;
  }

  /**
   * 后退 / 前进：只挪游标，不产生新记录。
   *
   * **先导航、成功了才挪游标**：失败时游标若先跑了，显示的内容与游标就对不上，
   * "前进"再也回不来。
   */
  async function goBack(): Promise<void> {
    const tab = active.value;
    if (!tab || tab.cursor <= 0) {
      return;
    }
    const target = tab.history[tab.cursor - 1];
    if (target === undefined) {
      return;
    }
    if (await navigate(target, "history")) {
      tab.cursor -= 1;
    }
  }

  async function goForward(): Promise<void> {
    const tab = active.value;
    if (!tab || tab.cursor >= tab.history.length - 1) {
      return;
    }
    const target = tab.history[tab.cursor + 1];
    if (target === undefined) {
      return;
    }
    if (await navigate(target, "history")) {
      tab.cursor += 1;
    }
  }

  return {
    tabs,
    activeIndex,
    active,
    closed,
    committed,
    shakeTick,
    canGoBack,
    canGoForward,
    canReopen,
    select,
    newTab,
    newTabWith,
    close,
    closeOthers,
    reopenClosed,
    move,
    navigate,
    goBack,
    goForward,
  };
}
