import type { UsageDay, UsageTotals } from '@/contracts/types';

/**
 * 用量页的数字与日期格式化。集中放在这里，不在组件里内联判断：
 * 同一批数字会同时出现在指标卡、图表峰值、表格与柱状图的 title 里，口径必须一致。
 *
 * 全是纯函数，不依赖 i18n 的模块级状态——文案由页面用 `t()` 拼。需要本地化的格式化
 * （千分位、日期时间）显式收 `locale`，这样断言不依赖跑测试的机器语言。
 */

/** 紧凑 token 数字。K/M/B 是开发者的通用记法，且与界面语言无关，所以不本地化成「万/亿」。 */
export function compactTokens(value: number): string {
  if (!Number.isFinite(value) || value <= 0) return '0';
  if (value < 1_000) return String(Math.round(value));
  if (value < 1_000_000) return `${trim(value / 1_000, 1)}K`;
  if (value < 1_000_000_000) return `${trim(value / 1_000_000, 1)}M`;
  return `${trim(value / 1_000_000_000, 2)}B`;
}

/** 带千分位的完整数字。给 `title` 用：卡片上紧凑，悬停能看到准确值。 */
export function exactTokens(value: number, locale: string): string {
  return new Intl.NumberFormat(locale).format(Math.round(value));
}

/**
 * 计划窗口时长：≥ 24 小时按天说，否则按小时说。
 *
 * 真实数据里窗口不都是天——本机按时间戳取到的最新一条是 300 分钟（5 小时窗口），
 * 写死「{days} 天」会渲染成「窗口 0 天」。
 */
export function planWindowLabel(minutes: number): {
  key: 'usage.planWindowDays' | 'usage.planWindowHours';
  vars: Record<string, string>;
} {
  if (minutes >= 1_440) {
    return { key: 'usage.planWindowDays', vars: { days: trim(minutes / 1_440, 1) } };
  }
  return { key: 'usage.planWindowHours', vars: { hours: trim(minutes / 60, 1) } };
}

/** 已用百分比取整：小数位在这里没有意义，只会让一行文字变长。 */
export function usedPercentLabel(percent: number): string {
  return percentLabel(percent);
}

/**
 * 百分比取整并夹在 0..100。与已用百分比同一条规则，供占比、命中率这类口径一致的地方用：
 * 页面上所有百分数（命中率、两条构成比、两张排行的占比）必须同一口径，否则同一件事会读出两个数。
 */
export function percentLabel(percent: number): string {
  return Number.isFinite(percent) ? String(Math.round(clampPercent(percent))) : '0';
}

/** 夹在 0..100 的百分比数值。直接喂给条的宽度（`width: {n}%`）——越界的宽度会被算成溢出。 */
export function clampPercent(percent: number): number {
  if (!Number.isFinite(percent)) return 0;
  return Math.min(100, Math.max(0, percent));
}

/** Unix 秒 → 本地日期时间。重置时间要能一眼对上手表的日期。 */
export function localDateTime(unixSeconds: number, locale: string): string {
  return new Intl.DateTimeFormat(locale, {
    year: 'numeric', month: '2-digit', day: '2-digit', hour: '2-digit', minute: '2-digit',
  }).format(new Date(unixSeconds * 1000));
}

/** 图表横轴用的短日期：`MM-DD`。 */
export function shortDate(date: string): string {
  return date.length >= 10 ? date.slice(5) : date;
}

/** 图表峰值。全部为零时返回 0——图表按零绘制，不编一个高度出来。 */
export function peakTotal(days: UsageDay[]): number {
  return days.reduce((max, day) => Math.max(max, day.totals.totalTokens), 0);
}

/* ---- 结论带与图表的派生值 ---- */

/**
 * 日均：总 Token ÷ 请求的天数。
 *
 * 分母用 `rangeDays`（请求的范围）而不是 `daily.length`：报告契约保证 daily 逐日零填充，
 * 两者本来就相等；用 rangeDays 是为了在 data 被裁剪的报告上也不会把日均算大。
 * 天数非正时返回 0，不做除法。
 */
export function avgPerDay(total: number, rangeDays: number): number {
  return rangeDays > 0 ? total / rangeDays : 0;
}

/** 每会话均量。会话为 0 时返回 0——没有会话就没有「每次」可言。 */
export function perSession(total: number, sessions: number): number {
  return sessions > 0 ? total / sessions : 0;
}

/** 缓存命中率（%）：缓存读取 ÷ 输入。输入为 0 时返回 0，不做 0/0。 */
export function cacheRate(totals: UsageTotals): number {
  return totals.inputTokens > 0 ? (totals.cachedTokens / totals.inputTokens) * 100 : 0;
}

/**
 * 占比（%）：分项 ÷ 本范围总量。
 *
 * 两张排行与构成条都走这一个函数，所以「按模型 46%」与「按供应商 61%」是同一个分母下的数，
 * 能横着比。分母为 0 时返回 0。
 */
export function sharePercent(part: number, whole: number): number {
  return whole > 0 ? (part / whole) * 100 : 0;
}

/**
 * 峰值日：总量最大的那天。并列时取最早的一天（严格大于才替换）。
 * 范围内没有任何用量（或 daily 为空）时返回 null——不指定某一天当峰值，也不编一个日期出来。
 */
export function peakDay(days: UsageDay[]): UsageDay | null {
  const index = peakIndex(days);
  return index < 0 ? null : days[index]!;
}

/**
 * 峰值点在 `daily` 里的下标。规则与 `peakDay` 是同一条（并列取最早、全零不给点），
 * 折线图要的是下标（算横坐标），结论带要的是那一天本身，两处不能各写一遍。
 * 没有正用量时返回 -1。
 */
export function peakIndex(days: UsageDay[]): number {
  let best = -1;
  for (let index = 0; index < days.length; index++) {
    const value = days[index]!.totals.totalTokens;
    if (value > 0 && (best < 0 || value > days[best]!.totals.totalTokens)) best = index;
  }
  return best;
}

/** 热力图分档：0 = 没有记录，1..4 = 当日用量相对本范围峰值的四档。 */
export function heatLevel(value: number, peak: number): number {
  if (!(value > 0) || !(peak > 0)) return 0;
  const ratio = value / peak;
  if (ratio > 0.75) return 4;
  if (ratio > 0.5) return 3;
  if (ratio > 0.25) return 2;
  return 1;
}

/**
 * 热力图的列序：周一 = 0 … 周日 = 6。
 *
 * 用 `T00:00:00` 构造本地时间再取星期：`new Date('2026-09-25')` 会被当成 UTC 零点，
 * 东八区以下的时区会把日期挪到前一天、星期整整错一列。
 */
export function weekdayIndex(date: string): number {
  const day = new Date(`${date}T00:00:00`).getDay();
  return Number.isNaN(day) ? 0 : (day + 6) % 7;
}

/**
 * 热力网格的列数（一周一列）。首列可能只有后半周，所以要把第一天占掉的格数一起算进去：
 * offset + 天数 向上取整到整周。报告里的 daily 是逐日零填充的连续日期，日期数就是天数。
 * 空报告返回 0——不画网格，也不凭空给一列。
 */
export function heatColumns(days: UsageDay[]): number {
  const first = days[0];
  if (!first) return 0;
  return Math.ceil((weekdayIndex(first.date) + days.length) / 7);
}

/**
 * 热力格里第 index 天落在第几列第几行（行 1 = 周一 … 7 = 周日，列 1 = 第一周）。
 *
 * 每格都显式落在行列上，不靠 `grid-auto-flow` 的自动排布：那套规则在「首个元素显式占位」
 * 时的推进顺序各家实现细节多，显式定位没有解释空间，也便于测试直接核对坐标。
 */
export function heatCell(index: number, offset: number): { column: number; row: number } {
  const slot = offset + index;
  return { column: Math.floor(slot / 7) + 1, row: (slot % 7) + 1 };
}

/**
 * 热力图列头的星期窄名（中文「一」、英文「M」）。用固定的一周取名字，与"今天是星期几"无关。
 * 2026-01-05 是周一，所以 index 0..6 依次是周一到周日。
 */
export function weekdayNarrow(locale: string, index: number): string {
  return new Intl.DateTimeFormat(locale, { weekday: 'narrow' }).format(new Date(2026, 0, 5 + index));
}

/** 去掉小数末尾的 0：`7.0` → `7`，`3.35` 保留。 */
function trim(value: number, digits: number): string {
  return value.toFixed(digits).replace(/\.0+$/, '');
}
