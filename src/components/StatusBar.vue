<script setup lang="ts">
// 状态栏：引擎、识别资源、屏幕连接与讲解进度分别显示，并提供诊断信息与日志目录。
import { computed } from "vue";
import { NBadge, NButton, NPopover, NSpin, NTag, NTooltip, useMessage } from "naive-ui";
import { api, copyText, errorMessage, isTauri } from "../api/backend";
import { refreshStatus, wb } from "../state/workbench";

const message = useMessage();
const engine = computed(() => wb.status?.engine);
const stats = computed(() => wb.view?.stats);

async function restart() {
  await api.restartEngine().catch((e) => message.error(errorMessage(e)));
  await refreshStatus();
}

async function copyDiagnostics() {
  try {
    await copyText(await api.diagnostics());
    message.success("诊断信息已复制，可粘贴到问题反馈中");
  } catch (e) {
    message.error(errorMessage(e));
  }
}

async function openLogs() {
  await api.openLogDir().catch((e) => message.error(errorMessage(e)));
}

async function stopCapture() {
  await api.stopCapture().catch((e) => message.error(errorMessage(e)));
}
</script>

<template>
  <footer class="status">
    <span class="item" data-testid="engine-status">
      <template v-if="engine?.state === 'ready'">
        <n-tooltip>
          <template #trigger><n-tag size="small" type="success" :bordered="false">引擎就绪</n-tag></template>
          {{ engine.identity.name }} · {{ engine.identity.nnue }} · 评分 {{ engine.identity.score_type }}
          <div v-for="n in engine.notes" :key="n">{{ n }}</div>
        </n-tooltip>
      </template>
      <template v-else-if="engine?.state === 'starting'"><n-spin :size="12" /> 引擎启动中</template>
      <template v-else-if="engine?.state === 'failed'">
        <n-tooltip>
          <template #trigger><n-tag size="small" type="error" :bordered="false">引擎不可用</n-tag></template>
          {{ engine.reason }}
        </n-tooltip>
        <n-button size="tiny" quaternary @click="restart">重启引擎</n-button>
      </template>
      <template v-else>引擎已停止 <n-button size="tiny" quaternary @click="restart">启动</n-button></template>
    </span>

    <span class="item">
      <n-tooltip>
        <template #trigger>
          <n-tag size="small" :type="wb.status?.recognition.available ? 'default' : 'warning'" :bordered="false">
            识别{{ wb.status?.recognition.available ? "可用" : "不可用" }}
          </n-tag>
        </template>
        {{ wb.status?.recognition.model }}：{{ wb.status?.recognition.detail }}
      </n-tooltip>
    </span>

    <span v-if="wb.capture.state !== 'idle'" class="item">
      <template v-if="wb.capture.state === 'tracking'">屏幕记录中：{{ wb.capture.window.title }}</template>
      <template v-else-if="wb.capture.state === 'stopped'">屏幕连接已停止</template>
      <template v-else>屏幕连接等待确认</template>
      <n-button v-if="wb.capture.state !== 'stopped'" size="tiny" quaternary @click="stopCapture">停止</n-button>
    </span>

    <span v-if="stats && stats.moves" class="item muted" data-testid="explain-stats">
      讲解：详细 {{ stats.detailed }} / 共 {{ stats.moves }} 着
      <template v-if="stats.pending"> · 分析中 {{ stats.pending }}</template>
      <template v-if="stats.partial + stats.failed"> · 不完整 {{ stats.partial + stats.failed }}</template>
    </span>

    <span class="spacer" />

    <n-popover v-if="wb.status?.notices.length" trigger="click" placement="top-end">
      <template #trigger>
        <n-badge :value="wb.status.notices.length" :max="9">
          <n-button size="tiny" quaternary>提示</n-button>
        </n-badge>
      </template>
      <div class="notices">
        <p v-for="(n, i) in wb.status.notices" :key="i">{{ n }}</p>
      </div>
    </n-popover>
    <n-button size="tiny" quaternary @click="copyDiagnostics">复制诊断信息</n-button>
    <n-button v-if="isTauri" size="tiny" quaternary @click="openLogs">打开日志目录</n-button>
  </footer>
</template>

<style scoped>
.status {
  display: flex;
  align-items: center;
  gap: 14px;
  padding: 0 var(--sp-4) 6px;
  font-size: 12px;
  min-height: 30px;
  color: var(--text-muted);
}
.item {
  display: inline-flex;
  align-items: center;
  gap: 6px;
}
.muted {
  color: var(--text-muted);
}
.spacer {
  flex: 1;
}
.notices {
  max-width: 420px;
  max-height: 260px;
  overflow: auto;
  font-size: 13px;
}
.notices p {
  margin: 4px 0;
}
</style>
