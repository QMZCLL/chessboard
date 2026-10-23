<script setup lang="ts">
// 设置：时间在界面上以秒填写，保存时只在这里换算一次为整数毫秒。
import { reactive, ref, watch } from "vue";
import { NButton, NDivider, NDrawer, NDrawerContent, NForm, NFormItem, NInput, NInputNumber, NSelect, NSwitch, NTag, useMessage } from "naive-ui";
import { api, errorMessage } from "../api/backend";
import type { AppConfig, Audience, CoachProvider, CoachScope, Detail } from "../api/types";
import { refreshCoach, wb } from "../state/workbench";
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
  coach_enabled: false,
  coach_provider: "deepseek" as CoachProvider,
  coach_base_url: "",
  coach_model: "",
  coach_timeout_s: 60,
  coach_max_tokens: 1500,
  coach_parallel: 2,
  coach_budget: 200,
  coach_scope: "off" as CoachScope,
  coach_audience: "beginner" as Audience,
  coach_detail: "normal" as Detail,
});
// 密钥单独保存，不随设置回传；输入框只用于填写新密钥
const coachKey = ref("");
const savingKey = ref(false);

const PROVIDERS = [
  { label: "DeepSeek（JSON 输出）", value: "deepseek" },
  { label: "OpenAI（结构化输出）", value: "openai" },
  { label: "本机 Ollama", value: "ollama" },
];
const DEFAULT_URL: Record<CoachProvider, string> = {
  deepseek: "https://api.deepseek.com",
  openai: "https://api.openai.com/v1",
  ollama: "http://127.0.0.1:11434",
};
const SCOPES = [
  { label: "只在点击时生成", value: "off" },
  { label: "查看某一着时自动生成", value: "viewed" },
];
const AUDIENCES = [
  { label: "入门", value: "beginner" },
  { label: "进阶", value: "intermediate" },
  { label: "高级", value: "advanced" },
];
const DETAILS = [
  { label: "简短", value: "brief" },
  { label: "常规（约 200～350 字）", value: "normal" },
  { label: "详细", value: "deep" },
];

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
  form.coach_enabled = c.coach.enabled;
  form.coach_provider = c.coach.provider;
  form.coach_base_url = c.coach.base_url;
  form.coach_model = c.coach.model;
  form.coach_timeout_s = c.coach.timeout_ms / 1000;
  form.coach_max_tokens = c.coach.max_output_tokens;
  form.coach_parallel = c.coach.max_parallel_requests;
  form.coach_budget = c.coach.daily_request_budget;
  form.coach_scope = c.coach.auto_explain_scope;
  form.coach_audience = c.coach.audience;
  form.coach_detail = c.coach.detail;
}

async function saveKey(clear = false) {
  savingKey.value = true;
  try {
    wb.status = await api.setCoachCredential(clear ? null : coachKey.value);
    coachKey.value = "";
    void refreshCoach();
    message.success(clear ? "已清除密钥" : "密钥已保存");
  } catch (e) {
    message.error(errorMessage(e));
  } finally {
    savingKey.value = false;
  }
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
    coach: {
      ...base.coach,
      enabled: form.coach_enabled,
      provider: form.coach_provider,
      base_url: form.coach_base_url.trim(),
      model: form.coach_model.trim(),
      timeout_ms: secondsToMs(form.coach_timeout_s),
      max_output_tokens: form.coach_max_tokens,
      max_parallel_requests: form.coach_parallel,
      daily_request_budget: form.coach_budget,
      auto_explain_scope: form.coach_scope,
      audience: form.coach_audience,
      detail: form.coach_detail,
    },
  };
  saving.value = true;
  try {
    wb.status = await api.updateConfig(config);
    load(wb.status.config);
    void refreshCoach();
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

        <n-divider title-placement="left">AI 讲解（可选）</n-divider>
        <n-form-item label="启用 AI 讲解"><n-switch v-model:value="form.coach_enabled" data-testid="coach-enabled" /></n-form-item>
        <n-form-item label="模型服务"><n-select v-model:value="form.coach_provider" :options="PROVIDERS" /></n-form-item>
        <n-form-item label="服务地址">
          <n-input v-model:value="form.coach_base_url" :placeholder="`留空使用 ${DEFAULT_URL[form.coach_provider]}`" />
        </n-form-item>
        <n-form-item label="模型名称">
          <n-input v-model:value="form.coach_model" :placeholder="form.coach_provider === 'ollama' ? '本机已下载的模型' : '如 deepseek-chat'" />
        </n-form-item>
        <n-form-item v-if="form.coach_provider !== 'ollama'" label="密钥">
          <div class="key">
            <n-input v-model:value="coachKey" type="password" show-password-on="click" placeholder="填写新密钥" />
            <n-button size="small" :disabled="!coachKey.trim()" :loading="savingKey" @click="saveKey()">保存</n-button>
            <n-tag v-if="wb.status?.coach.key_configured" size="small" type="success" :bordered="false">已配置</n-tag>
            <n-button v-if="wb.status?.coach.key_configured" size="small" quaternary @click="saveKey(true)">清除</n-button>
          </div>
        </n-form-item>
        <n-form-item label="自动生成"><n-select v-model:value="form.coach_scope" :options="SCOPES" /></n-form-item>
        <n-form-item label="学习水平"><n-select v-model:value="form.coach_audience" :options="AUDIENCES" /></n-form-item>
        <n-form-item label="详细程度"><n-select v-model:value="form.coach_detail" :options="DETAILS" /></n-form-item>
        <n-form-item label="超时 (秒)"><n-input-number v-model:value="form.coach_timeout_s" :min="2" :max="300" :step="5" /></n-form-item>
        <n-form-item label="最大输出长度"><n-input-number v-model:value="form.coach_max_tokens" :min="300" :max="8000" :step="100" /></n-form-item>
        <n-form-item label="并发请求数"><n-input-number v-model:value="form.coach_parallel" :min="1" :max="4" /></n-form-item>
        <n-form-item label="每日调用上限">
          <n-input-number v-model:value="form.coach_budget" :min="0" :max="100000" :step="50" />
        </n-form-item>
        <p class="muted">
          开启后，查看的这一着的局面、着法历史与分析结果会发送到所选的模型服务（Ollama 在本机处理）。密钥只保存在本机后台，不会显示在界面上。
          每日调用上限填 0 表示不限；今天已调用 {{ wb.status?.coach.used_today ?? 0 }} 次。
        </p>

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
.key {
  display: flex;
  gap: 6px;
  align-items: center;
  width: 100%;
}
.key :deep(.n-input) {
  flex: 1;
  min-width: 140px;
}
</style>
