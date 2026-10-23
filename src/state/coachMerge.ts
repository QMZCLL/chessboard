// AI 讲解状态的合并：按后台给出的状态版本判断新旧，不依赖响应到达的先后。
// 证据或设置改变时状态会变而记录本身不变（例如“已生成”变为“已过期”），记录时间不能用来判断新旧。

import type { CoachView } from "../api/types";

/** 两份状态中较新的一份：版本更大的为准；版本相同时内容一致，采用后到达的。 */
export function newerCoach(current: CoachView | undefined, incoming: CoachView): CoachView {
  if (!current) return incoming;
  return incoming.seq >= current.seq ? incoming : current;
}
