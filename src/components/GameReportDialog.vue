<script setup lang="ts">
// 整局报告：程序选出的关键局面（可以回到棋盘、加入复习）、值得保留的判断与需要重新观察的地方。
// 筛选与统计都由程序完成，文字取自已经生成的讲解；评价只比较每一着自己的候选，不同局面的评分不直接相减。
import { computed, ref, watch } from "vue";
import { NButton, NModal, NSpin, NTag, useMessage } from "naive-ui";
import { api, errorMessage } from "../api/backend";
import type { GameReport, KeyPosition } from "../api/types";
import { addToReview } from "../state/review";
import { STATUS_LABEL } from "../state/reviewList";
import { openStudy, selectNode, study, wb } from "../state/workbench";

const message = useMessage();
const report = ref<GameReport | null>(null);
const loading = ref(false);
const error = ref<string | null>(null);
/** 报告请求的代次：关闭报告、切换会话或重新打开后，较早发出的请求返回时不再采用 */
let seq = 0;

watch(
  () => [wb.dialogs.report, study.value?.id ?? null] as const,
  async ([open, sid]) => {
    const id = ++seq;
    if (!open || !sid) {
      loading.value = false;
      return;
    }
    // 打开的是另一盘棋的报告时，不先显示上一盘的内容
    if (report.value?.study_id !== sid) report.value = null;
    loading.value = true;
    error.value = null;
    try {
      const r = await api.gameReport(sid);
      if (id === seq) report.value = r;
    } catch (e) {
      if (id === seq) error.value = errorMessage(e);
    } finally {
      if (id === seq) loading.value = false;
    }
  },
);

const practiceText = computed(() => {
  const p = report.value?.practice;
  if (!p?.practiced) return "这盘棋还没有先想后讲的练习记录";
  return `先想后讲练习了 ${p.practiced} 着：第一步与棋谱一致 ${p.matched} 着，用了提示 ${p.hinted} 着，没有走子就看讲解 ${p.revealed} 着`;
});
const notInReview = computed(() => report.value?.key_positions.filter((k) => !k.review) ?? []);

/** 在棋盘上查看：总是报告所属的会话中的这一着 */
async function view(nodeId: number) {
  const r = report.value;
  if (!r) return;
  wb.dialogs.report = false;
  if (study.value?.id === r.study_id) await selectNode(nodeId);
  else await openStudy(r.study_id, nodeId);
}

async function add(k: KeyPosition) {
  const s = report.value;
  if (!s) return;
  try {
    k.review = (await addToReview(s.study_id, k.node_id)).card.status;
  } catch (e) {
    message.error(errorMessage(e));
  }
}

async function addAll() {
  const list = notInReview.value;
  for (const k of list) await add(k);
  message.success(`已把 ${list.length} 处关键局面加入复习`);
}
</script>

<template>
  <n-modal v-model:show="wb.dialogs.report" preset="card" title="本局报告" style="width: min(760px, 96vw)" data-testid="report-dialog">
    <div v-if="loading" class="loading"><n-spin size="small" /> 正在整理…</div>
    <p v-else-if="error" class="error">{{ error }}</p>
    <div v-else-if="report" class="report">
      <p class="muted" data-testid="report-overview">
        {{ report.study_name }} · 主线 {{ report.moves }} 着，其中 {{ report.detailed }} 着有完整的引擎分析；{{ practiceText }}。
      </p>

      <h4>本局关键局面</h4>
      <ol v-if="report.key_positions.length" class="keys" data-testid="report-keys">
        <li v-for="k in report.key_positions" :key="k.node_id" :data-report-node="k.node_id">
          <div class="key-head">
            <strong>{{ k.label }}</strong>
            <span class="spacer" />
            <n-button size="tiny" quaternary @click="view(k.node_id)">在棋盘上查看</n-button>
            <n-tag v-if="k.review" size="small" :bordered="false">复习中 · {{ STATUS_LABEL[k.review] }}</n-tag>
            <n-button v-else size="tiny" secondary data-testid="report-add" @click="add(k)">加入复习</n-button>
          </div>
          <div class="reasons">
            <n-tag v-for="r in k.reasons" :key="r" size="small" :bordered="false" class="reason">{{ r }}</n-tag>
          </div>
          <p v-if="k.summary" class="summary">{{ k.summary }}</p>
        </li>
      </ol>
      <p v-else class="muted">这盘棋中还没有达到条件的关键局面。</p>
      <n-button v-if="notInReview.length > 1" size="small" type="primary" data-testid="report-add-all" @click="addAll">
        把这 {{ notInReview.length }} 处加入复习
      </n-button>

      <h4>值得保留的判断</h4>
      <ul v-if="report.good_choices.length" class="items" data-testid="report-good">
        <li v-for="g in report.good_choices" :key="g.node_id">
          <span class="muted">{{ g.label }}</span> · {{ g.text }}
        </li>
      </ul>
      <p v-else class="muted">这盘棋中还没有先想后讲中选对或与首选接近的记录。</p>

      <h4>需要重新观察的地方</h4>
      <ul v-if="report.to_recheck.length" class="items" data-testid="report-recheck">
        <li v-for="r in report.to_recheck" :key="r.node_id">
          <span class="muted">{{ r.label }}</span> · {{ r.text }}
          <n-button text size="tiny" type="primary" @click="view(r.node_id)">查看</n-button>
        </li>
      </ul>
      <p v-else class="muted">没有需要重新检查的评价问题或练习中的选择。</p>

      <ul class="notes" data-testid="report-notes">
        <li v-for="n in report.notes" :key="n">{{ n }}</li>
      </ul>
    </div>
  </n-modal>
</template>

<style scoped>
.report h4 {
  margin: var(--sp-4) 0 var(--sp-2);
  font-size: 14px;
}
.keys,
.items {
  margin: 0;
  padding-left: 20px;
  line-height: 1.65;
}
.keys li + li {
  margin-top: var(--sp-3);
}
.key-head {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 6px;
}
.spacer {
  flex: 1;
}
.reasons {
  display: flex;
  flex-wrap: wrap;
  gap: 4px;
  margin-top: 4px;
}
.summary {
  margin: 4px 0 0;
}
.notes {
  margin: var(--sp-4) 0 0;
  padding-left: 18px;
  font-size: var(--fs-meta);
  color: var(--text-muted);
}
.muted {
  color: var(--text-muted);
}
.error {
  color: var(--accent-red);
}
.loading {
  display: flex;
  align-items: center;
  gap: var(--sp-2);
  color: var(--text-muted);
}
</style>
