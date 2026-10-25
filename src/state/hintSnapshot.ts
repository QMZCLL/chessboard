// 先想后讲的提示快照：第一次打开提示时记下当时显示的内容，三级提示都来自这份快照。
// 练习期间分析结果更新（例如候选着法变了）也不改变已经看到的提示；练习记录保存的就是看到的内容。
import type { ThinkHints, UsedHints } from "../api/types";

/** 复制一份提示，之后的分析更新不影响它。 */
export function snapshotHints(hints: ThinkHints | null | undefined): ThinkHints | null {
  return hints ? (JSON.parse(JSON.stringify(hints)) as ThinkHints) : null;
}

/** 用到的前几级提示：一级是局面需求，二级加上相关棋子，三级加上可以考虑的着法。 */
export function usedHints(hints: ThinkHints | null, level: number): UsedHints | null {
  if (!hints || level <= 0) return null;
  return {
    observe: hints.observe,
    focus: level >= 2 ? hints.focus : null,
    moves: level >= 3 ? hints.moves : [],
  };
}
