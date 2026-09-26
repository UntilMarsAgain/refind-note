//! 大图查看器的开关状态。
//!
//! 只有"有没有、是哪一张"两个值，所以用两个函数加一个 ref 就够，不必上事件总线。
//! 全局只挂一份查看器（在 App 根上），阅读视图与编辑器预览点开的是同一个。
import { ref } from "vue";

export interface ViewingImage {
  /** 已经解析好的取件地址（点击时用的是 DOM 上的 src） */
  url: string;
  alt: string;
}

export const viewingImage = ref<ViewingImage | null>(null);

export function viewImage(url: string, alt: string) {
  viewingImage.value = { url, alt };
}

export function closeImage() {
  viewingImage.value = null;
}
