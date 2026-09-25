import type { UsageDay } from '@/contracts/types';
import { exactTokens, shortDate } from './usagePolicy';
import styles from './UsagePage.module.css';

/**
 * 每日 Token 柱状图。手绘 SVG——本仓库没有图表依赖，也不为一张柱状图新增下载面。
 *
 * 三个决定：
 * - **单序列**（每天的总 Token）。三色堆叠（未缓存输入 / 缓存读取 / 输出）在 90 根柱子上
 *   会糊成一片，而且 `total != input + output` 是真实存在的（约 9% 的事件有差异），
 *   堆叠会给人「加起来正好是总量」的错误印象。分栏留在指标卡与表格里。
 * - **零值也占位**：横轴按天连续，零值画一根浅色细线。柱子数量随时间范围变化，
 *   中间空一天不能让两侧看起来相邻。
 * - **文字不放 SVG 里**：峰值与横轴日期用 HTML 渲染，字号、行高与字体栈都跟着 token 走
 *   （尤其是等宽栈的中文回退），对比度也能被审计脚本按真实底色量到。
 *
 * 文案不进这个组件：峰值那一行由页面用 `t()` 拼好后当 `peakLabel` 传进来。
 */
export function UsageTrendChart({ days, ariaLabel, peakLabel, locale }: {
  days: UsageDay[];
  /** 整图的可访问名称；逐日数值由每根柱子的 `<title>` 提供。 */
  ariaLabel: string;
  /** 已本地化的峰值文案（含数字）。 */
  peakLabel: string;
  locale: string;
}) {
  const peak = days.reduce((max, day) => Math.max(max, day.totals.totalTokens), 0);
  // 柱子的横向槽位固定 12 个单位、间隙 3，viewBox 宽度随数据点数量增长。
  // preserveAspectRatio="none" 让柱子横向拉伸铺满容器——柱子是矩形，拉伸不变形。
  const slot = 12;
  const gap = 3;
  const plotHeight = 100;
  const minVisibleHeight = 1.5;
  const width = Math.max(days.length * slot, slot);
  const first = days[0]?.date ?? '';
  const last = days[days.length - 1]?.date ?? '';

  return (
    <div className={styles.chart}>
      <div className={styles.chartTop}>
        <span className={styles.chartPeak}>{peakLabel}</span>
      </div>
      <svg
        className={styles.chartSvg}
        viewBox={`0 0 ${width} ${plotHeight}`}
        preserveAspectRatio="none"
        role="img"
        aria-label={ariaLabel}
      >
        <line className={styles.chartBaseline} x1="0" y1={plotHeight} x2={width} y2={plotHeight} />
        {days.map((day, index) => {
          const value = day.totals.totalTokens;
          const height = peak > 0 ? (value / peak) * (plotHeight - 4) : 0;
          const drawn = value > 0 ? Math.max(height, minVisibleHeight) : minVisibleHeight;
          return (
            <rect
              key={day.date}
              className={value > 0 ? styles.chartBar : styles.chartBarZero}
              x={index * slot + gap / 2}
              y={plotHeight - drawn}
              width={slot - gap}
              height={drawn}
            >
              <title>{`${day.date} · ${exactTokens(value, locale)}`}</title>
            </rect>
          );
        })}
      </svg>
      <div className={styles.chartAxis}>
        <span>{shortDate(first)}</span>
        <span>{shortDate(last)}</span>
      </div>
    </div>
  );
}
