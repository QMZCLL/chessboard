// 界面主题：跟随系统、浅色或深色。属于本机的显示偏好，保存在浏览器存储中（不可用时只是不记住选择）。

import { computed, ref, watchEffect } from "vue";

export type ThemePref = "system" | "light" | "dark";

const KEY = "xq-theme";

function load(): ThemePref {
  try {
    const v = localStorage.getItem(KEY);
    return v === "light" || v === "dark" ? v : "system";
  } catch {
    return "system";
  }
}

export const themePref = ref<ThemePref>(load());

const media = typeof window !== "undefined" ? window.matchMedia("(prefers-color-scheme: dark)") : null;
const systemDark = ref(media?.matches ?? false);
media?.addEventListener("change", (e) => (systemDark.value = e.matches));

export const resolvedTheme = computed<"light" | "dark">(() =>
  themePref.value === "system" ? (systemDark.value ? "dark" : "light") : themePref.value,
);

watchEffect(() => {
  document.documentElement.dataset.theme = resolvedTheme.value;
});

export function setThemePref(pref: ThemePref) {
  themePref.value = pref;
  try {
    localStorage.setItem(KEY, pref);
  } catch {
    // 记不住主题不影响使用
  }
}
