//   Refind Note is a note-taking software.
//   Copyright (C) 2026 Until Mars Again
//
//   This program is free software: you can redistribute it and/or modify
//   it under the terms of the GNU Affero General Public License as published by
//   the Free Software Foundation, either version 3 of the License, or
//   (at your option) any later version.
//
//   This program is distributed in the hope that it will be useful,
//   but WITHOUT ANY WARRANTY; without even the implied warranty of
//   MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
//   GNU Affero General Public License for more details.
//
//   You should have received a copy of the GNU Affero General Public License
//   along with this program.  If not, see <http://www.gnu.org/licenses/>.

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
