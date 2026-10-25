// 试走对比结果的新旧：同一组走法可能先后在不同的分析条件下比较，“已经完成”不代表“属于最新的请求”。
// 每次请求有递增的序号，推送的完整结果沿用请求时的序号；只有同一次请求的完整结果能代替它尚未完成的结果。
import type { CompareView } from "../api/types";

const keyOf = (v: CompareView) => `${v.study_id}:${v.node_id}:${v.mine}:${v.reference}`;

/** 当前显示的结果与新到达的结果（请求的响应或推送）中应当显示的一份。 */
export function newerCompare(current: CompareView | null, incoming: CompareView): CompareView {
  // 换了比较对象：采用新的
  if (!current || keyOf(current) !== keyOf(incoming)) return incoming;
  if (incoming.seq !== current.seq) return incoming.seq > current.seq ? incoming : current;
  // 同一次请求：推送的完整结果先到时，不用随后到达的尚未完成的响应覆盖它
  return current.pending || !incoming.pending ? incoming : current;
}
