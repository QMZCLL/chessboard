<script setup lang="ts">
import { computed } from "vue";
import { NConfigProvider, NDialogProvider, NMessageProvider, darkTheme, useOsTheme, zhCN, dateZhCN } from "naive-ui";
import Workbench from "./components/Workbench.vue";

const os = useOsTheme();
const theme = computed(() => (os.value === "dark" ? darkTheme : null));
</script>

<template>
  <n-config-provider :theme="theme" :locale="zhCN" :date-locale="dateZhCN">
    <n-message-provider placement="top-right">
      <n-dialog-provider>
        <Workbench />
      </n-dialog-provider>
    </n-message-provider>
  </n-config-provider>
</template>
