import { useState, type CSSProperties } from 'react';
import type { UsageDay } from '@/contracts/types';
import { t } from '@/i18n';
import {
  compactTokens, exactTokens, heatCell, heatColumns, heatLevel, peakIndex, peakTotal, shortDate,
  weekdayIndex, weekdayNarrow,
} from './usagePolicy';
import styles from './UsagePage.module.css';

/**
 * 每日 Token 用量的三种画法：日历热力图、柱状图、折线图。手绘 DIV / SVG——
 * 本仓库没有图表依赖，也不为三张图新增下载面。
 *
 * 三者共用同一套读数：卡片头部那行**悬停读数**（`chartReadout`）给出「哪一天 · 用了多少 ·
 * 几个会话」，没有悬停时退回整个范围的起止日期。以前只有热力格上的原生 `title` 与柱状图
 * 的浮层提示，慢一拍、盖住图形，而且折线上根本没有；同一份数据换一种画法就得换一套读法，
 * 读出来的数字还容易对不上。现在图形怎么换，读数都在同一个位置、同一种写法。
 *
 * 三个决定沿用上一版：
 * - **单序列**（每天的总 Token）。三色堆叠（未缓存输入 / 缓存读取 / 输出）在 90 根柱子上
 *   会糊成一片，而且 `total != input + output` 是真实存在的（约 9% 的事件有差异），
 *   堆叠会给人「加起来正好是总量」的错误印象。分栏留在指标卡与表格里。
 * - **零值也占位**：横轴按天连续，零值在柱状图里画一根浅色细线、在折线里落在基线上，
 *   中间空一天不能让两侧看起来相邻。
 * - **悬停只指出「是哪一天」**：柱子上提亮、折线上画竖向准线 + 圆点、热力格加描边，
 *   数字一律由读数那行承担，不在图形上再压一层会挡住数据的提示条。
 *
 * 文案：读数里的会话数在这里用 `t()` 拼（只有悬停时才需要，不值得为它拉平接口）；
 * 峰值那行与整图的可访问名称仍由页面拼好传进来。
 *
 * 三种画法都挂同一对钩子：`data-date`（这一列/格是哪一天）与 `data-hovered`（读数正在说的那一天）。
 * 命中区、提亮、描边、准线在三种画法里长得完全不一样，钩子让「现在读的是哪一天」有一条统一问法，
 * 测试与版面审计都按它断言，不必去猜某个 class 名或坐标。
 */

export type DailyChartKind = 'heat' | 'bar' | 'line';

export function UsageTrendChart({ kind, days, ariaLabel, peakLabel, locale }: {
  kind: DailyChartKind;
  days: UsageDay[];
  /** 整图的可访问名称（按画法给：热力图 / 柱状图 / 折线图），逐日数值由读数与热力格的 title 提供。 */
  ariaLabel: string;
  /** 已本地化的峰值文案（含数字）。 */
  peakLabel: string;
  locale: string;
}) {
  const [hovered, setHovered] = useState<number | null>(null);
  const peak = peakTotal(days);
  const first = days[0];
  const last = days[days.length - 1];
  const hoveredDay = hovered != null ? days[hovered] : undefined;
  const plot = { days, peak, ariaLabel, hovered, onHover: setHovered, locale };

  return <div className={styles.chart} data-chart={kind}>
    <div className={styles.chartMeta}>
      {/* 读数：悬停给出那一天，没有悬停时给整个范围。两者行高相同、同一行同一套等宽字，
          鼠标扫过时这一行不会换位置，也不会把下面的图形顶下去。 */}
      <span
        className={styles.chartReadout}
        title={hoveredDay ? exactTokens(hoveredDay.totals.totalTokens, locale) : undefined}
      >
        {hoveredDay
          ? `${shortDate(hoveredDay.date)} · ${compactTokens(hoveredDay.totals.totalTokens)} · ${t('usage.trendSessions', { count: hoveredDay.sessions })}`
          : `${first ? shortDate(first.date) : ''} – ${last ? shortDate(last.date) : ''}`}
      </span>
      <span className={styles.chartPeak}>{peakLabel}</span>
    </div>
    {kind === 'heat' && <HeatGrid {...plot} />}
    {kind === 'bar' && <BarPlot {...plot} />}
    {kind === 'line' && <LinePlot {...plot} />}
  </div>;
}

/** 三个图形共用的输入：同一批天数、同一个峰值口径、同一条悬停状态。 */
interface PlotProps {
  days: UsageDay[];
  peak: number;
  ariaLabel: string;
  hovered: number | null;
  onHover: (index: number | null) => void;
  locale: string;
}

/**
 * 日历热力图（GitHub 贡献图那副形状）：**7 行 = 周一…周日，一列 = 一周**。
 *
 * 四个决定：
 * - **列宽吃满卡片**（`repeat(N, minmax(0, 1fr))` + 方格 `aspect-ratio: 1`）：列数一多，
 *   格子就随卡片变窄变密，一年 53 列正好铺满整行。列少时（30 天只有 5 列）不给格子
 *   无限放大的机会——`--heat-cell-max` 封顶，否则 5 个 200px 的方块会像五个头像。
 * - **每格显式落在行列上**，不用 `grid-auto-flow` 的自动排布：第一列可能只有后半周，
 *   自动排布在「首个元素显式占位」时各家实现的推进顺序不好赌，显式定位没有解释空间。
 * - **第一格按星期占位**（`heatCell`），不补造不存在的日期格：补出来的格子长得跟
 *   「这天没有用量」一模一样，会把一个不存在的日期画成测得的零。
 * - **分档相对本范围的峰值**（`heatLevel`），不用绝对阈值：7 天与 365 天的量级差两个数量级，
 *   写死阈值只会让一整个范围糊在同一档里。
 */
function HeatGrid({ days, peak, ariaLabel, hovered, onHover, locale }: PlotProps) {
  const first = days[0];
  const offset = first ? weekdayIndex(first.date) : 0;
  // 空报告给 0 列，但 `repeat(0, …)` 是无效值（整条声明会被丢掉），所以这里至少写 1：
  // 没有格子要画，列数只影响那一格的轨道宽度。
  const columns = Math.max(1, heatColumns(days));
  return <>
    <div
      className={styles.heatGrid}
      role="img"
      aria-label={ariaLabel}
      style={{ '--heat-cols': String(columns) } as CSSProperties}
      onMouseLeave={() => onHover(null)}
    >
      {/* 星期名只标周一 / 周三 / 周五（GitHub 也是这么标的）：7 行全标时，窄窗口下 8px 的行距
          放不下 12px 的字，会挤成一团。行盒高度在样式里记成 0，行高由正方格子决定。 */}
      {[0, 2, 4].map(row => <span
        key={row}
        className={styles.heatWeekday}
        aria-hidden="true"
        style={{ gridColumn: 1, gridRow: row + 1 }}
      >{weekdayNarrow(locale, row)}</span>)}
      {days.map((day, index) => {
        const cell = heatCell(index, offset);
        return <div
          key={day.date}
          className={styles.heatCell}
          data-level={heatLevel(day.totals.totalTokens, peak)}
          data-date={day.date}
          data-hovered={index === hovered ? 'true' : undefined}
          style={{ gridColumn: cell.column + 1, gridRow: cell.row }}
          title={`${day.date} · ${exactTokens(day.totals.totalTokens, locale)} · ${t('usage.trendSessions', { count: day.sessions })}`}
          onMouseEnter={() => onHover(index)}
        />;
      })}
    </div>
    {/* 图例：少 → 四档 → 多。空档不画进图例，它与「有用量但很少」是两回事。 */}
    <div className={styles.heatLegend}>
      <span>{t('usage.heatLegendLow')}</span>
      {[1, 2, 3, 4].map(level => <span key={level} className={styles.legendSwatch} data-level={level} />)}
      <span>{t('usage.heatLegendHigh')}</span>
    </div>
  </>;
}

/**
 * 柱状图：普通 DIV（不是 SVG）。柱子用百分比高度、圆角顶、悬停提亮都是一行 CSS 的事。
 * 整列都是命中区（比柱子本身宽得多），鼠标不用瞄准；悬停的回应是提亮而不是变色，
 * 数字由头部读数给出。天数一多柱子会细到只剩一条线，所以这一档只在短期范围里好用。
 */
function BarPlot({ days, peak, ariaLabel, hovered, onHover }: PlotProps) {
  return <div className={styles.chartPlot} role="img" aria-label={ariaLabel} onMouseLeave={() => onHover(null)}>
    {days.map((day, index) => {
      const value = day.totals.totalTokens;
      // 高度按峰值取百分比：0 也画一根 2px 的细线占住这一列。
      const heightPct = peak > 0 && value > 0 ? Math.max((value / peak) * 100, 2) : 0;
      return <div
        key={day.date}
        className={styles.chartSlot}
        data-date={day.date}
        onMouseEnter={() => onHover(index)}
      >
        <div
          className={value > 0 ? styles.chartBar : styles.chartBarZero}
          data-hovered={index === hovered ? 'true' : undefined}
          style={value > 0 ? { height: `${heightPct}%` } : undefined}
        />
      </div>;
    })}
  </div>;
}

/**
 * 折线图：手写 SVG，**坐标用百分比、不套 `viewBox`**。
 *
 * 套了 `viewBox` 又要铺满卡片就得 `preserveAspectRatio="none"`，那会把描边和圆点一起拉扁
 * （`vector-effect` 只救描边，救不了多边形与圆点）。百分比坐标不受缩放影响：图形跟着卡片
 * 拉伸，线的粗细与峰值点的圆度始终是设计值。逐段画 `<line>` 而不是一条 `<polyline>`——
 * `points` 不接受百分比，`d` 里的百分比各家解析历史不一，分段是最没有解释空间的写法，
 * 端点的圆头线帽会把接缝盖住。
 *
 * 命中的地方仍是整列的透明命中区（与柱状图同一套），数据点本身不做点击目标（3px 太细）。
 * 悬停时画一条竖向准线 + 一个圆点：读数说数字，准线与圆点说「是这一天这个值」。
 */
function LinePlot({ days, peak, ariaLabel, hovered, onHover }: PlotProps) {
  const count = days.length;
  /** 第 index 天的横坐标（%）：取这一列的中点，与命中区、准线、圆点同一套算法。 */
  const x = (index: number) => ((index + 0.5) / Math.max(1, count)) * 100;
  /** 纵坐标（%）：0 值落在基线上（折线不能像柱子那样抬 2%，那会把零读成「有一点」）。 */
  const y = (value: number) => (peak > 0 && value > 0 ? Math.max((value / peak) * 100, 0) : 0);
  const at = peakIndex(days);
  const hoveredDay = hovered != null ? days[hovered] : undefined;

  return <div className={styles.linePlot} role="img" aria-label={ariaLabel} onMouseLeave={() => onHover(null)}>
    <svg className={styles.lineSvg} aria-hidden="true" focusable="false">
      {days.slice(1).map((day, index) => <line
        key={day.date}
        className={styles.lineStroke}
        x1={`${x(index)}%`} y1={`${100 - y(days[index]!.totals.totalTokens)}%`}
        x2={`${x(index + 1)}%`} y2={`${100 - y(day.totals.totalTokens)}%`}
      />)}
    </svg>
    {/* 峰值点常驻：折线的最高处是这一档最该被一眼看到的位置。数字在头部读数里（`usage.trendPeak`）。 */}
    {at >= 0 && <span
      className={styles.linePeakDot}
      data-peak={days[at]!.date}
      aria-hidden="true"
      style={{ left: `${x(at)}%`, bottom: `${y(days[at]!.totals.totalTokens)}%` }}
    />}
    <div className={styles.lineHits}>
      {days.map((day, index) => <div
        key={day.date}
        className={styles.lineHit}
        data-date={day.date}
        onMouseEnter={() => onHover(index)}
      />)}
    </div>
    {hoveredDay && hovered != null && <>
      <span className={styles.lineGuide} data-hovered="true" aria-hidden="true" style={{ left: `${x(hovered)}%` }} />
      <span
        className={styles.lineMark}
        aria-hidden="true"
        style={{ left: `${x(hovered)}%`, bottom: `${y(hoveredDay.totals.totalTokens)}%` }}
      />
    </>}
  </div>;
}
