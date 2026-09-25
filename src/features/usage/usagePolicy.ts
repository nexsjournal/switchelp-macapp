import type { UsageDay } from '@/contracts/types';

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
  if (!Number.isFinite(percent)) return '0';
  return String(Math.min(100, Math.max(0, Math.round(percent))));
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

/** 去掉小数末尾的 0：`7.0` → `7`，`3.35` 保留。 */
function trim(value: number, digits: number): string {
  return value.toFixed(digits).replace(/\.0+$/, '');
}
