/**
 * 看大图：全局一份状态。
 *
 * 正文里的图片点一下就看大图 —— 这件事与"在哪一页"无关，所以状态放在模块一级，
 * 界面由 `App.vue` 挂一次。
 */

import { readonly, ref } from "vue";

export interface ViewingImage {
    /** 图片的真实地址（`refind://…` 或 `https://…`） */
    url: string;
    /** 说明文字（通常是 alt） */
    alt: string;
}

const viewing = ref<ViewingImage | null>(null);

export const viewingImage = readonly(viewing);

export function viewImage(url: string, alt = ""): void {
    viewing.value = { url, alt };
}

export function closeImage(): void {
    viewing.value = null;
}
