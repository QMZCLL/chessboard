// 稳定性等级的文字与标签颜色（讲解、候选比较、试走对比共用）。
import type { StabilityLevel } from "../api/types";

export const STABILITY: Record<StabilityLevel, { text: string; type: "success" | "default" | "warning" | "info" }> = {
  high: { text: "高稳定", type: "success" },
  medium: { text: "中等稳定", type: "default" },
  shifting: { text: "排名仍在变化", type: "warning" },
  incomplete: { text: "分析不完整", type: "default" },
};
