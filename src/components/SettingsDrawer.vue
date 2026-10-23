<script setup lang="ts">
// 设置：按用户任务分组（学习与显示、AI 讲解、分析引擎、屏幕连接、诊断），不常用的参数收在“高级”中。
// 时间在界面上以秒填写，保存时只在这里换算一次为整数毫秒。主题只影响本机显示，选择后立即生效；
// 密钥使用独立的保存流程，其他设置在底部统一保存，并显示是否有未保存的修改。
import { computed, reactive, ref, watch } from "vue";
import {
  NButton,
  NCollapse,
  NCollapseItem,
  NDrawer,
  NDrawerContent,
  NForm,
  NFormItem,
  NInput,
  NInputNumber,
  NRadioButton,
  NRadioGroup,
  NSelect,
  NSwitch,
  NTabPane,
  NTabs,
  NTag,
  useMessage,
} from "naive-ui";
import { api, copyText, errorMessage, isTauri } from "../api/backend";
import type { AppConfig, Audience, CoachProvider, CoachScope, Detail } from "../api/types";
import { refreshCoach, wb, type SettingsTab } from "../state/workbench";
import { setThemePref, themePref } from "../state/theme";
import { viewport } from "../state/layout";
import { secondsToMs } from "../utils/units";

const message = useMessage();
const saving = ref(false);
const tab = ref<SettingsTab>("learn");
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
// 最近一次载入或保存时的表单，用来判断是否有未保存的修改
const savedForm = ref("");
const dirty = computed(() => savedForm.value !== "" && JSON.stringify(form) !== savedForm.value);
// 密钥单独保存，不随设置回传；输入框只用于填写新密钥
const coachKey = ref("");
const savingKey = ref(false);

const THEMES = [
  { label: "跟随系统", value: "system" },
  { label: "浅色", value: "light" },
  { label: "深色", value: "dark" },
];
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
  form.confidence_threshold = Math.round(c.capture.confidence_threshold * 100) / 100;
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
  savedForm.value = JSON.stringify(form);
}

async function saveKey(clear = false) {
  savingKey.value = true;
  try {
    wb.status = await api.setCoachCredential(clear ? null : coachKey.value);
    coachKey.value = "";
    void refreshCoach();
    const done = clear ? "已清除密钥" : "密钥已更新";
    message.success(dirty.value ? `${done}；其他修改尚未保存` : done);
  } catch (e) {
    message.error(errorMessage(e));
  } finally {
    savingKey.value = false;
  }
}

watch(
  () => wb.dialogs.settings,
  (open) => {
    if (!open) return;
    if (wb.status) load(wb.status.config);
    if (wb.settingsTab) tab.value = wb.settingsTab;
    wb.settingsTab = null;
  },
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
  // 引擎参数在正在进行的分析结束后才生效，只在改了它们时提示
  const ENGINE_KEYS = ["quick_time_ms", "quick_depth", "deep_time_ms", "deep_depth", "multipv", "repetition_rule", "sixty_move_rule"] as const;
  const engineChanged =
    base.engine.threads !== config.engine.threads ||
    base.engine.hash_mb !== config.engine.hash_mb ||
    ENGINE_KEYS.some((k) => base.analysis[k] !== config.analysis[k]);
  saving.value = true;
  try {
    wb.status = await api.updateConfig(config);
    load(wb.status.config);
    void refreshCoach();
    message.success(engineChanged ? "设置已保存；引擎设置在当前分析结束后生效" : "设置已保存");
  } catch (e) {
    message.error(errorMessage(e));
  } finally {
    saving.value = false;
  }
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

const engineText = computed(() => {
  const e = wb.status?.engine;
  if (!e) return "";
  if (e.state === "ready") return `可用 · ${e.identity.name}`;
  if (e.state === "failed") return `不可用：${e.reason}`;
  return e.state === "starting" ? "启动中" : "已停止";
});
</script>

<template>
  <n-drawer v-model:show="wb.dialogs.settings" :width="Math.min(480, viewport)" placement="right">
    <n-drawer-content title="设置" closable :native-scrollbar="false">
      <n-tabs v-model:value="tab" type="line" size="small" class="tabs">
        <n-tab-pane name="learn" tab="学习与显示">
          <n-form label-placement="left" label-width="112" size="small">
            <n-form-item label="界面主题">
              <n-radio-group :value="themePref" size="small" @update:value="setThemePref">
                <n-radio-button v-for="t in THEMES" :key="t.value" :value="t.value">{{ t.label }}</n-radio-button>
              </n-radio-group>
            </n-form-item>
            <p class="muted">主题只影响本机显示，选择后立即生效。</p>
            <n-form-item label="走子后自动讲解"><n-switch v-model:value="form.auto_explain" /></n-form-item>
            <n-form-item label="学习水平"><n-select v-model:value="form.coach_audience" :options="AUDIENCES" /></n-form-item>
            <n-form-item label="AI 讲解篇幅"><n-select v-model:value="form.coach_detail" :options="DETAILS" /></n-form-item>
            <p class="muted">学习水平与篇幅用于 AI 讲解；修改后，已有的 AI 讲解会标为过期。</p>
          </n-form>
        </n-tab-pane>

        <n-tab-pane name="coach" tab="AI 讲解">
          <n-form label-placement="left" label-width="112" size="small">
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
                <n-button size="small" :disabled="!coachKey.trim()" :loading="savingKey" @click="saveKey()">
                  {{ wb.status?.coach.key_configured ? "更换" : "保存" }}
                </n-button>
              </div>
            </n-form-item>
            <div v-if="form.coach_provider !== 'ollama'" class="key-state">
              <n-tag v-if="wb.status?.coach.key_configured" size="small" type="success" :bordered="false">已配置</n-tag>
              <n-tag v-else size="small" :bordered="false">未配置</n-tag>
              <n-button v-if="wb.status?.coach.key_configured" size="tiny" quaternary @click="saveKey(true)">清除密钥</n-button>
              <span class="muted">密钥单独保存在本机后台，不会显示在界面上。</span>
            </div>
            <n-form-item label="自动生成"><n-select v-model:value="form.coach_scope" :options="SCOPES" /></n-form-item>
            <n-collapse class="advanced">
              <n-collapse-item title="高级" name="advanced">
                <n-form-item label="超时 (秒)"><n-input-number v-model:value="form.coach_timeout_s" :min="2" :max="300" :step="5" /></n-form-item>
                <n-form-item label="最大输出长度"><n-input-number v-model:value="form.coach_max_tokens" :min="300" :max="8000" :step="100" /></n-form-item>
                <n-form-item label="并发请求数"><n-input-number v-model:value="form.coach_parallel" :min="1" :max="4" /></n-form-item>
                <n-form-item label="每日调用上限"><n-input-number v-model:value="form.coach_budget" :min="0" :max="100000" :step="50" /></n-form-item>
              </n-collapse-item>
            </n-collapse>
            <p class="muted">
              开启后，查看的这一着的局面、着法历史与分析结果会发送到所选的模型服务（Ollama 在本机处理）。
              每日调用上限填 0 表示不限；今天已调用 {{ wb.status?.coach.used_today ?? 0 }} 次。
            </p>
          </n-form>
        </n-tab-pane>

        <n-tab-pane name="engine" tab="分析引擎">
          <n-form label-placement="left" label-width="112" size="small">
            <n-form-item label="快速分析 (秒)">
              <n-input-number v-model:value="form.quick_s" :min="0.1" :max="60" :step="0.1" :precision="1" data-testid="quick-seconds" />
            </n-form-item>
            <n-form-item label="深入分析 (秒)">
              <n-input-number v-model:value="form.deep_s" :min="0.2" :max="600" :step="0.5" :precision="1" data-testid="deep-seconds" />
            </n-form-item>
            <n-form-item label="候选数量"><n-input-number v-model:value="form.multipv" :min="1" :max="5" /></n-form-item>
            <n-collapse class="advanced">
              <n-collapse-item title="高级" name="advanced">
                <n-form-item label="快速分析深度"><n-input-number v-model:value="form.quick_depth" :min="1" :max="100" /></n-form-item>
                <n-form-item label="深入分析深度"><n-input-number v-model:value="form.deep_depth" :min="1" :max="100" /></n-form-item>
                <n-form-item label="线程数"><n-input-number v-model:value="form.threads" :min="1" :max="256" /></n-form-item>
                <n-form-item label="哈希表 (MB)"><n-input-number v-model:value="form.hash_mb" :min="16" :max="65536" :step="64" /></n-form-item>
                <n-form-item label="重复局面规则"><n-select v-model:value="form.repetition_rule" :options="RULES" /></n-form-item>
                <n-form-item label="六十步自然限着"><n-switch v-model:value="form.sixty_move_rule" /></n-form-item>
              </n-collapse-item>
              <n-collapse-item title="云库（可选）" name="cloud">
                <n-form-item label="启用云库查询"><n-switch v-model:value="form.cloud_enabled" /></n-form-item>
                <n-form-item label="云库超时 (秒)"><n-input-number v-model:value="form.cloud_timeout_s" :min="0.5" :max="60" :step="0.5" :precision="1" /></n-form-item>
                <p class="muted">开启后，点击“查询云库”会把当前局面发送到 chessdb.cn。云库结果只作参考，不与本地分析分数相减。</p>
              </n-collapse-item>
            </n-collapse>
          </n-form>
        </n-tab-pane>

        <n-tab-pane name="capture" tab="屏幕连接">
          <n-form label-placement="left" label-width="112" size="small">
            <n-form-item label="截图间隔 (毫秒)"><n-input-number v-model:value="form.interval_ms" :min="50" :max="5000" :step="50" /></n-form-item>
            <n-form-item label="稳定帧数"><n-input-number v-model:value="form.stable_frames" :min="1" :max="30" /></n-form-item>
            <n-form-item label="确认前等待帧数"><n-input-number v-model:value="form.unexplained_frames" :min="0" :max="100" /></n-form-item>
            <n-form-item label="置信度提示阈值">
              <n-input-number v-model:value="form.confidence_threshold" :min="0.3" :max="0.99" :step="0.05" :precision="2" />
            </n-form-item>
            <n-form-item label="识别模型文件"><n-input v-model:value="form.model" placeholder="留空使用自带模型（修改后需重启）" /></n-form-item>
          </n-form>
        </n-tab-pane>

        <n-tab-pane name="diag" tab="诊断">
          <div class="diag">
            <p><span class="muted">引擎：</span>{{ engineText }}</p>
            <p>
              <span class="muted">屏幕识别：</span>{{ wb.status?.recognition.available ? "可用" : "不可用" }}
              <span class="muted">（{{ wb.status?.recognition.detail }}）</span>
            </p>
            <p><span class="muted">AI 讲解：</span>{{ wb.status?.coach.ready ? `可用 · ${wb.status.coach.model_label}` : wb.status?.coach.reason }}</p>
            <p class="muted">数据目录：{{ wb.status?.data_dir }}</p>
            <div class="diag-actions">
              <n-button size="small" @click="copyDiagnostics">复制诊断信息</n-button>
              <n-button v-if="isTauri" size="small" @click="openLogs">打开日志目录</n-button>
            </div>
            <n-form label-placement="left" label-width="112" size="small">
              <n-form-item label="日志级别"><n-select v-model:value="form.log_level" :options="LEVELS" /></n-form-item>
            </n-form>
          </div>
        </n-tab-pane>
      </n-tabs>
      <template #footer>
        <div class="footer">
          <span class="save-state" :class="{ dirty }" data-testid="settings-state">{{ dirty ? "有未保存的修改" : "所有修改已保存" }}</span>
          <n-button type="primary" :loading="saving" :disabled="!dirty" data-testid="save-settings" @click="save">保存设置</n-button>
        </div>
      </template>
    </n-drawer-content>
  </n-drawer>
</template>

<style scoped>
.tabs :deep(.n-tab-pane) {
  padding-top: var(--sp-3);
}
.muted {
  color: var(--text-muted);
  font-size: 12px;
  margin: 0 0 var(--sp-2);
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
.key-state {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 6px;
  margin: -6px 0 var(--sp-3) 112px;
}
.key-state .muted {
  margin: 0;
}
.advanced {
  margin: var(--sp-1) 0 var(--sp-3);
}
.diag p {
  margin: 0 0 var(--sp-2);
  font-size: var(--fs-group);
}
.diag-actions {
  display: flex;
  gap: var(--sp-2);
  margin: var(--sp-2) 0 var(--sp-4);
}
.footer {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--sp-3);
  width: 100%;
}
.save-state {
  font-size: var(--fs-aux);
  color: var(--text-muted);
}
.save-state.dirty {
  color: var(--accent-amber);
  font-weight: 600;
}
</style>
