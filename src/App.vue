<script setup lang="ts">
import { computed } from "vue";
import { NConfigProvider, NDialogProvider, NMessageProvider, darkTheme, zhCN, dateZhCN } from "naive-ui";
import type { GlobalThemeOverrides } from "naive-ui";
import Workbench from "./components/Workbench.vue";
import { resolvedTheme } from "./state/theme";

// 组件库与自定义组件使用同一套主题：主题由 state/theme.ts 决定，颜色与 theme.css 中的变量一致
const theme = computed(() => (resolvedTheme.value === "dark" ? darkTheme : null));

const FONT =
  '-apple-system, "Segoe UI", "PingFang SC", "HarmonyOS Sans SC", "MiSans", "Microsoft YaHei UI", "Microsoft YaHei", "Noto Sans SC", "Noto Sans CJK SC", sans-serif';

const shared = {
  borderRadius: "8px",
  borderRadiusSmall: "6px",
  fontFamily: FONT,
  fontWeightStrong: "600",
  heightSmall: "30px",
  heightMedium: "34px",
};

// 组件库的表面、文字与边框颜色与 theme.css 的纸面配色一致
const light = {
  ...shared,
  primaryColor: "#0f766e",
  primaryColorHover: "#0d8a80",
  primaryColorPressed: "#0b5f58",
  primaryColorSuppl: "#0d8a80",
  infoColor: "#1d4ed8",
  infoColorHover: "#2a5ce6",
  infoColorPressed: "#1a43bb",
  infoColorSuppl: "#2a5ce6",
  warningColor: "#9a4508",
  errorColor: "#b3261e",
  successColor: "#276b43",
  textColorBase: "#231f19",
  textColor1: "#231f19",
  textColor2: "#3a342b",
  textColor3: "#6b6356",
  placeholderColor: "#a39a8b",
  bodyColor: "#ece6da",
  cardColor: "#fbf9f4",
  modalColor: "#fbf9f4",
  popoverColor: "#fffdf9",
  tableColor: "#fbf9f4",
  inputColor: "#fffdf9",
  actionColor: "#f4efe5",
  tagColor: "#efe9dd",
  hoverColor: "rgba(110, 84, 40, 0.07)",
  borderColor: "rgba(84, 62, 28, 0.2)",
  dividerColor: "rgba(84, 62, 28, 0.12)",
  boxShadow2: "0 3px 8px -2px rgba(60, 40, 10, 0.12), 0 12px 32px -8px rgba(60, 40, 10, 0.2)",
  boxShadow3: "0 6px 16px -4px rgba(60, 40, 10, 0.16), 0 24px 56px -12px rgba(60, 40, 10, 0.3)",
};

const dark = {
  ...shared,
  primaryColor: "#2bb3a6",
  primaryColorHover: "#3cc7b9",
  primaryColorPressed: "#20968b",
  primaryColorSuppl: "#3cc7b9",
  infoColor: "#7aa2ff",
  infoColorHover: "#93b4ff",
  infoColorPressed: "#5f8bf0",
  infoColorSuppl: "#93b4ff",
  warningColor: "#f5a524",
  errorColor: "#f2726a",
  successColor: "#5cc98a",
  textColorBase: "#ecebee",
  textColor1: "#ecebee",
  textColor2: "#d3d0d8",
  textColor3: "#9a95a2",
  bodyColor: "#141518",
  cardColor: "#1c1d21",
  modalColor: "#1c1d21",
  popoverColor: "#26282d",
  tableColor: "#1c1d21",
  inputColor: "#23252a",
  actionColor: "#23252a",
  tagColor: "#2a2c31",
  hoverColor: "rgba(255, 255, 255, 0.06)",
  borderColor: "rgba(255, 255, 255, 0.13)",
  dividerColor: "rgba(255, 255, 255, 0.08)",
};

const components = {
  // 按钮高度：最小的也不低于 24px 的点击目标；棋盘与讲解中的主要操作使用 36px
  Button: { fontWeight: "500", heightTiny: "26px", heightSmall: "32px", heightMedium: "36px" },
  Tabs: { tabFontWeightActive: "600", tabFontSizeSmall: "14px" },
  Tag: { borderRadius: "6px" },
  Card: { borderRadius: "12px" },
  Dialog: { borderRadius: "12px" },
  Alert: { borderRadius: "8px" },
};

const overrides = computed<GlobalThemeOverrides>(() => ({
  common: resolvedTheme.value === "dark" ? dark : light,
  ...components,
}));
</script>

<template>
  <n-config-provider :theme="theme" :theme-overrides="overrides" :locale="zhCN" :date-locale="dateZhCN">
    <!-- 提示出现在顶栏下方，不遮挡顶栏按钮 -->
    <n-message-provider placement="top-right" :container-style="{ top: '62px' }">
      <n-dialog-provider>
        <Workbench />
      </n-dialog-provider>
    </n-message-provider>
  </n-config-provider>
</template>
