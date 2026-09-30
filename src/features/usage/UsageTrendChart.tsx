import { useState } from 'react';
import type { UsageDay } from '@/contracts/types';
import { exactTokens, shortDate } from './usagePolicy';
import styles from './UsagePage.module.css';

import { t } from '@/i18n';

/**
 * 每日 Token 柱状图。手绘 DIV——本仓库没有图表依赖，也不为一张柱状图新增下载面。
 *
 * 三个决定：
 * - **单序列**（每天的总 Token）。三色堆叠（未缓存输入 / 缓存读取 / 输出）在 90 根柱子上
 *   会糊成一片，而且 `total != input + output` 是真实存在的（约 9% 的事件有差异），
 *   堆叠会给人「加起来正好是总量」的错误印象。分栏留在指标卡与表格里。
 * - **零值也占位**：横轴按天连续，零值画一根浅色细线。柱子数量随时间范围变化，
 *   中间空一天不能让两侧看起来相邻。
 * - **悬停有回应**：整列都是命中区（比柱子本身宽得多），悬停时柱子提亮，并在柱顶
 *   弹出日期 / 用量 / 会话数。以前只有 SVG 的原生 `<title>`，慢一拍还跟不上鼠标；
 *   HTML 提示条让「看某天用了多少」变成一次移动就够。
 *
 * 文案：提示条里的会话数在这里用 `t()` 拼（它在悬停时才需要，不值得为它拉平接口）；
 * 峰值那行仍由页面拼好当 `peakLabel` 传进来。
 */
export function UsageTrendChart({ days, ariaLabel, peakLabel, locale }: {
  days: UsageDay[];
  /** 整图的可访问名称；逐日数值由悬停提示提供。 */
  ariaLabel: string;
  /** 已本地化的峰值文案（含数字）。 */
  peakLabel: string;
  locale: string;
}) {
  const [hovered, setHovered] = useState<number | null>(null);
  const peak = days.reduce((max, day) => Math.max(max, day.totals.totalTokens), 0);
  const first = days[0]?.date ?? '';
  const last = days[days.length - 1]?.date ?? '';
  const hoveredDay = hovered != null ? days[hovered] : undefined;

  return (
    <div className={styles.chart}>
      <div className={styles.chartTop}>
        <span className={styles.chartPeak}>{peakLabel}</span>
      </div>
      <div className={styles.chartPlot} role="img" aria-label={ariaLabel}
        onMouseLeave={() => setHovered(null)}>
        {days.map((day, index) => {
          const value = day.totals.totalTokens;
          // 高度按峰值取百分比：0 也画一根 2px 的细线占住这一列。
          const heightPct = peak > 0 && value > 0 ? Math.max((value / peak) * 100, 2) : 0;
          return (
            <div key={day.date} className={styles.chartSlot}
              onMouseEnter={() => setHovered(index)}>
              <div className={value > 0 ? styles.chartBar : styles.chartBarZero}
                style={value > 0 ? { height: `${heightPct}%` } : undefined} />
            </div>
          );
        })}
        {hoveredDay && <div className={styles.chartTip} aria-hidden="true"
          style={{
            left: `clamp(56px, ${((hovered! + 0.5) / days.length) * 100}%, calc(100% - 56px))`,
            bottom: `calc(${hoveredDay.totals.totalTokens > 0
              ? Math.max((hoveredDay.totals.totalTokens / peak) * 100, 2) : 0}% + 8px)`,
          }}>
          <span className={styles.chartTipDate}>{shortDate(hoveredDay.date)}</span>
          <span className={styles.chartTipValue}>{exactTokens(hoveredDay.totals.totalTokens, locale)}</span>
          <span className={styles.chartTipMeta}>{t('usage.trendSessions', { count: hoveredDay.sessions })}</span>
        </div>}
      </div>
      <div className={styles.chartAxis}>
        <span>{shortDate(first)}</span>
        <span>{shortDate(last)}</span>
      </div>
    </div>
  );
}
