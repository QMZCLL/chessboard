/** 界面以秒填写的时间换算为后台使用的整数毫秒（只在保存设置时换算一次）。 */
export function secondsToMs(seconds: number | null): number {
  return Math.round((seconds ?? 0) * 1000);
}
