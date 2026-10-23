<script setup lang="ts">
// 设置：时间在界面上以秒填写，保存时只在这里换算一次为整数毫秒。
import { reactive, ref, watch } from "vue";
import { NButton, NDivider, NDrawer, NDrawerContent, NForm, NFormItem, NInput, NInputNumber, NSelect, NSwitch, useMessage } from "naive-ui";
import { api, errorMessage } from "../api/backend";
import type { AppConfig } from "../api/types";
import { wb } from "../state/workbench";
import { secondsToMs } from "../utils/units";

const message = useMessage();
const saving = ref(false);
const form = reactive({
  threads: 2,
  hash_mb: 128,
  quick_s: 1,
  quick_depth: 20,
  deep_s: 5,
  deep_depth: 30,
  multipv: 3,
  repetition_rule: "ChineseRule",
  sixty_move_rule: true,
  auto_explain: true,
  interval_ms: 150,
  stable_frames: 3,
  unexplained_frames: 6,
  confidence_threshold: 0.8,
  model: "",
  cloud_enabled: false,
  cloud_timeout_s: 5,
  log_level: "info",
});

const RULES = [
  { label: "中国象棋协会规则（ChineseRule）", value: "ChineseRule" },
  { label: "亚洲规则（AsianRule）", value: "AsianRule" },
  { label: "天天象棋规则（SkyRule）", value: "SkyRule" },
  { label: "计算机规则（ComputerRule）", value: "ComputerRule" },
  { label: "弈天规则（YitianRule）", value: "YitianRule" },
  { label: "允许长捉（AllowChase）", value: "AllowChase" },
];
const LEVELS = ["error", "warn", "info", "debug", "trace"].map((v) => ({ label: v, value: v }));

function load(c: AppConfig) {
  form.threads = c.engine.threads;
  form.hash_mb = c.engine.hash_mb;
  form.quick_s = c.analysis.quick_time_ms / 1000;
  form.quick_depth = c.analysis.quick_depth;
  form.deep_s = c.analysis.deep_time_ms / 1000;
  form.deep_depth = c.analysis.deep_depth;
  form.multipv = c.analysis.multipv;
  form.repetition_rule = c.analysis.repetition_rule;
  form.sixty_move_rule = c.analysis.sixty_move_rule;
  form.auto_explain = c.analysis.auto_explain;
  form.interval_ms = c.capture.interval_ms;
  form.stable_frames = c.capture.stable_frames;
  form.unexplained_frames = c.capture.unexplained_frames;
  form.confidence_threshold = c.capture.confidence_threshold;
  form.model = c.capture.model ?? "";
  form.cloud_enabled = c.cloud.enabled;
  form.cloud_timeout_s = c.cloud.timeout_ms / 1000;
  form.log_level = c.log_level;
}

watch(
  () => wb.dialogs.settings,
  (open) => open && wb.status && load(wb.status.config),
);

async function save() {
  const base = wb.status?.config;
  if (!base) return;
  const config: AppConfig = {
    ...base,
    log_level: form.log_level,
    engine: { ...base.engine, threads: form.threads, hash_mb: form.hash_mb },
    analysis: {
      quick_time_ms: secondsToMs(form.quick_s),
      quick_depth: form.quick_depth,
      deep_time_ms: secondsToMs(form.deep_s),
      deep_depth: form.deep_depth,
      multipv: form.multipv,
      repetition_rule: form.repetition_rule,
      sixty_move_rule: form.sixty_move_rule,
      auto_explain: form.auto_explain,
    },
    capture: {
      interval_ms: form.interval_ms,
      stable_frames: form.stable_frames,
      unexplained_frames: form.unexplained_frames,
      confidence_threshold: form.confidence_threshold,
      model: form.model.trim() || null,
    },
    cloud: { enabled: form.cloud_enabled, timeout_ms: secondsToMs(form.cloud_timeout_s) },
  };
  saving.value = true;
  try {
    wb.status = await api.updateConfig(config);
    load(wb.status.config);
    message.success("设置已保存；引擎设置在当前分析结束后生效");
  } catch (e) {
    message.error(errorMessage(e));
  } finally {
    saving.value = false;
  }
}
</script>

<template>
  <n-drawer v-model:show="wb.dialogs.settings" :width="420" placement="right">
    <n-drawer-content title="设置" closable>
      <n-form label-placement="left" label-width="120" size="small">
        <n-divider title-placement="left">引擎</n-divider>
        <n-form-item label="线程数"><n-input-number v-model:value="form.threads" :min="1" :max="256" /></n-form-item>
        <n-form-item label="哈希表 (MB)"><n-input-number v-model:value="form.hash_mb" :min="16" :max="65536" :step="64" /></n-form-item>
        <n-form-item label="重复局面规则"><n-select v-model:value="form.repetition_rule" :options="RULES" /></n-form-item>
        <n-form-item label="六十步自然限着"><n-switch v-model:value="form.sixty_move_rule" /></n-form-item>

        <n-divider title-placement="left">讲解分析预算</n-divider>
        <n-form-item label="快速分析 (秒)">
          <n-input-number v-model:value="form.quick_s" :min="0.1" :max="60" :step="0.1" :precision="1" data-testid="quick-seconds" />
        </n-form-item>
        <n-form-item label="快速分析深度"><n-input-number v-model:value="form.quick_depth" :min="1" :max="100" /></n-form-item>
        <n-form-item label="深入分析 (秒)">
          <n-input-number v-model:value="form.deep_s" :min="0.2" :max="600" :step="0.5" :precision="1" data-testid="deep-seconds" />
        </n-form-item>
        <n-form-item label="深入分析深度"><n-input-number v-model:value="form.deep_depth" :min="1" :max="100" /></n-form-item>
        <n-form-item label="候选数量"><n-input-number v-model:value="form.multipv" :min="1" :max="5" /></n-form-item>
        <n-form-item label="走子后自动讲解"><n-switch v-model:value="form.auto_explain" /></n-form-item>

        <n-divider title-placement="left">屏幕识别</n-divider>
        <n-form-item label="截图间隔 (毫秒)"><n-input-number v-model:value="form.interval_ms" :min="50" :max="5000" :step="50" /></n-form-item>
        <n-form-item label="稳定帧数"><n-input-number v-model:value="form.stable_frames" :min="1" :max="30" /></n-form-item>
        <n-form-item label="确认前等待帧数"><n-input-number v-model:value="form.unexplained_frames" :min="0" :max="100" /></n-form-item>
        <n-form-item label="置信度提示阈值">
          <n-input-number v-model:value="form.confidence_threshold" :min="0.3" :max="0.99" :step="0.05" :precision="2" />
        </n-form-item>
        <n-form-item label="识别模型文件"><n-input v-model:value="form.model" placeholder="留空使用自带模型（修改后需重启）" /></n-form-item>

        <n-divider title-placement="left">云库（可选）</n-divider>
        <n-form-item label="启用云库查询"><n-switch v-model:value="form.cloud_enabled" /></n-form-item>
        <n-form-item label="云库超时 (秒)"><n-input-number v-model:value="form.cloud_timeout_s" :min="0.5" :max="60" :step="0.5" :precision="1" /></n-form-item>
        <p class="muted">开启后，点击“查询云库”会把当前局面发送到 chessdb.cn。云库结果只作参考，不与本地分析分数相减。</p>

        <n-divider title-placement="left">日志</n-divider>
        <n-form-item label="日志级别"><n-select v-model:value="form.log_level" :options="LEVELS" /></n-form-item>
      </n-form>
      <template #footer>
        <n-button type="primary" :loading="saving" data-testid="save-settings" @click="save">保存</n-button>
      </template>
    </n-drawer-content>
  </n-drawer>
</template>

<style scoped>
.muted {
  color: var(--text-muted);
  font-size: 12px;
  margin: 0 0 8px;
}
</style>
