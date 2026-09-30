import { useCallback, useEffect, useState } from 'react';
import { ChartColumn, RefreshCw } from 'lucide-react';
import type { UsageReport, UsageTotals } from '@/contracts/types';
import { type DesktopClient, toCoreError } from '@/desktop/client';
import { EmptyState } from '@/components/EmptyState';
import { SegmentedTabs } from '@/components/SegmentedTabs';
import { showToast } from '@/components/Toast';
import { t, useLocale } from '@/i18n';
import { UsageTrendChart, type DailyChartKind } from './UsageTrendChart';
import {
  avgPerDay, cacheRate, clampPercent, compactTokens, exactTokens, localDateTime, peakDay,
  peakTotal, percentLabel, perSession, planWindowLabel, sharePercent, shortDate,
} from './usagePolicy';
import styles from './UsagePage.module.css';

/**
 * 用量页。只读本机 Codex 会话记录，统计 Token 用量与本机计划额度窗口。
 *
 * 三件刻意不做的事（理由见 docs/design/07-usage-page.md）：
 * - **不显示成本**：本工具的模型是用户自定义的第三方模型，没有可靠的单价来源。
 *   按规范「不展示没有可靠来源的数字」，宁可只给 token 真值。
 * - **四个指标不可相加**：`缓存读取 ⊂ 输入`，且真实数据里 `total ≠ 输入 + 输出`
 *   （约 9% 的事件有 ±1~7 的差），所以每处都把它们并列展示，不出现任何等于号。
 * - **计划额度没有就是没有**：一条 `rate_limits` 都读不到时不显示这张卡（也不显示 0%），
 *   换成一行说明——只有官方计划账号的会话才记录这个窗口。
 *
 * 版面顺序是「先结论、再节奏、再构成、再排行」，最后一行证据。**每张卡各占一整行**：
 * 一天一格的热力图要铺满才有 GitHub 贡献图那种密度，按模型与按供应商的名字长度差得远，
 * 并排时总有一边空出一大块。四块各用不同的图形：大数字 + 派生小卡（结论带）、
 * 热力图 / 柱状图 / 折线图（使用节奏，可切换）、双行构成条（Token 构成）、环形进度（计划额度）。
 *
 * 界面里的数字没有一个是写死的：全部由 `usagePolicy` 从 `UsageReport` 现算。
 */

const RANGES = [
  { id: '7', days: 7, label: 'usage.range7' },
  { id: '30', days: 30, label: 'usage.range30' },
  { id: '90', days: 90, label: 'usage.range90' },
  { id: '365', days: 365, label: 'usage.range365' },
] as const;

type RangeId = (typeof RANGES)[number]['id'];

const CHART_KINDS = [
  { id: 'heat', label: 'usage.chartHeat' },
  { id: 'bar', label: 'usage.chartBar' },
  { id: 'line', label: 'usage.chartLine' },
] as const;

/**
 * 每档范围的默认画法：短期用柱状图（一天一根、当天多少一眼量得出来），90 天用折线图
 * （90 根柱子会细成一道栅栏，折线连起来才看得出走势），一年只有热力图
 * （365 个点没有可读性，而 53 列方格正好是 GitHub 贡献图那副形状）。
 * 用户点过切换之后就按点的走（`chartKind`），只在这一档画不出来时才退回默认。
 */
const DEFAULT_CHART: Record<RangeId, DailyChartKind> = { '7': 'bar', '30': 'bar', '90': 'line', '365': 'heat' };

export function UsagePage({ client }: { client: DesktopClient }) {
  const locale = useLocale();
  const [range, setRange] = useState<RangeId>('30');
  const [chart, setChart] = useState<DailyChartKind | null>(null);
  const [report, setReport] = useState<UsageReport | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState('');

  const load = useCallback(async (next: RangeId) => {
    setLoading(true);
    try {
      const result = await client.usageReport(RANGES.find(item => item.id === next)!.days);
      setReport(result);
      setError('');
    } catch (cause) {
      const core = toCoreError(cause);
      setError(core.safeDetails[0] ?? t(core.messageKey));
    } finally {
      setLoading(false);
    }
  }, [client]);

  useEffect(() => { void load(range); }, [load, range]);

  const rescan = async () => {
    await load(range);
    showToast(t('usage.rescanDone'));
  };

  const tabs = RANGES.map(item => ({ id: item.id, label: t(item.label) }));
  // 一年档只有热力图：柱状与折线在 365 天上没有可读性，索性不给这两个选项。
  const kinds = CHART_KINDS.filter(item => item.id === 'heat' || range !== '365');
  const chartKind = chart != null && kinds.some(item => item.id === chart) ? chart : DEFAULT_CHART[range];

  return <div className={styles.page}>
    <div className={styles.toolbar}>
      <SegmentedTabs
        tabs={tabs}
        active={range}
        onChange={id => setRange(id as RangeId)}
        ariaLabel={t('usage.trendTitle')}
      />
      <button type="button" onClick={() => void rescan()} disabled={loading}>
        <RefreshCw size={16} className={loading ? styles.spin : ''} />{t('usage.rescan')}
      </button>
    </div>

    {/* 页面级状态留在页面里：加载态与错误要一直看得见，直到状态本身改变。
        动作结果（已重新扫描）才走 Toast。 */}
    {error && <div className="error-message" role="alert">{error}</div>}

    {loading && !report
      ? <div className={styles.note}>{t('usage.loading')}</div>
      : report && report.scannedFiles === 0
        ? <EmptyState
            icon={ChartColumn}
            title={t('usage.emptyTitle')}
            description={t('usage.emptyDescription', { directory: report.sourceDirectory })}
            action={<button type="button" onClick={() => void rescan()}>{t('usage.rescan')}</button>}
          />
        : report && <>
          <Summary report={report} locale={locale} />

          {report.sessions === 0 && <div className={styles.note}>{t('usage.rangeEmpty', { days: report.rangeDays })}</div>}

          {/* 每日用量单独占一整行：热力图要铺满才有 GitHub 贡献图那种密度，
              柱状与折线也需要横向空间；与「Token 构成」并排时它只能分到半行。 */}
          <Rhythm
            report={report}
            kind={chartKind}
            kinds={kinds.map(item => ({ id: item.id, label: t(item.label) }))}
            onKindChange={id => setChart(id as DailyChartKind)}
            locale={locale}
          />

          <Composition totals={report.totals} locale={locale} />

          {/* 两张排行各占一整行：模型名与供应商名长度差得远，并排时短的那一边
              下方会空出一大块；整行之后比例条也更长，「谁占多少」量得更准。 */}
          <Ranking
            title={t('usage.modelTitle')}
            firstColumn={t('usage.colModel')}
            rows={report.byModel.map(row => ({ key: row.model, sessions: row.sessions, totals: row.totals }))}
            whole={report.totals.totalTokens}
            locale={locale}
            mono
          />
          <Ranking
            title={t('usage.providerTitle')}
            firstColumn={t('usage.colProvider')}
            rows={report.byProvider.map(row => ({ key: row.provider, sessions: row.sessions, totals: row.totals }))}
            whole={report.totals.totalTokens}
            locale={locale}
          />

          <PlanWindow plan={report.planWindow} locale={locale} />

          {/* 证据行：数字来自哪个目录、扫了多少文件、有多少读不了，都如实写出来。
              有不可读的文件时不能只字不提——那会让「合计」看起来比实际更完整。 */}
          <p className={styles.sourceLine}>
            {t('usage.sourceLine', { directory: report.sourceDirectory, files: report.scannedFiles })}
            {report.unreadableFiles > 0 && <> · <span className={styles.sourceWarn}>{t('usage.unreadable', { count: report.unreadableFiles })}</span></>}
          </p>
        </>}
  </div>;
}

/**
 * 结论带：左边是唯一的主数字（总 Token）与它的口径，右边是四个派生指标。
 *
 * 口径那行写的是「输入 / 输出 / 缓存读取」三个并列口径，而不是一句「= 输入 + 输出」：
 * 真实数据里 total 与两者之和本来就不相等（约 9% 的事件有差）。「缓存读取含在输入内」这条
 * 包含关系不在这里说——它由下面 Token 构成里那条深色子段连同 `usage.metricCachedNote` 交代，
 * 写在子段旁边才有指代对象。
 *
 * 四个派生指标全部现算。文案键本身带着数值（「日均 {value}」），所以每张卡是一句话而不是
 * 「标签 + 数字」两行：数字在这里是句子的宾语，不做成四个更大的数字去跟左边的主数字抢位置。
 * 紧凑记法之外都挂了完整值的 `title`，要准确数字时悬停就有；百分比本身已经是取整后的准确值，
 * 不再挂一个重复自己的 title。
 */
function Summary({ report, locale }: { report: UsageReport; locale: string }) {
  const { totals } = report;
  const avg = avgPerDay(totals.totalTokens, report.rangeDays);
  const per = perSession(totals.totalTokens, report.sessions);
  const rate = cacheRate(totals);
  const peak = peakDay(report.daily);

  const derived: Array<{ text: string; title?: string; note?: string }> = [
    {
      text: t('usage.avgPerDay', { value: compactTokens(avg) }),
      title: t('usage.avgPerDay', { value: exactTokens(avg, locale) }),
    },
    ...(peak ? [{
      text: t('usage.peakDay', { date: shortDate(peak.date), value: compactTokens(peak.totals.totalTokens) }),
      title: t('usage.peakDay', { date: peak.date, value: exactTokens(peak.totals.totalTokens, locale) }),
    }] : []),
    {
      text: t('usage.perSession', { value: compactTokens(per) }),
      title: t('usage.perSession', { value: exactTokens(per, locale) }),
    },
    {
      text: t('usage.cacheRate', { percent: percentLabel(rate) }),
      note: t('usage.cacheRateNote'),
    },
  ];

  return <section className={`${styles.card} ${styles.summary}`}>
    <div className={styles.hero}>
      <span className={styles.heroLabel}>{t('usage.metricTotal')}</span>
      <span className={styles.heroValue} title={exactTokens(totals.totalTokens, locale)}>{compactTokens(totals.totalTokens)}</span>
      <p className={styles.heroMeta}>
        <span>{t('usage.metricInput')} <span className={styles.heroNum} title={exactTokens(totals.inputTokens, locale)}>{compactTokens(totals.inputTokens)}</span></span>
        <span>{t('usage.metricOutput')} <span className={styles.heroNum} title={exactTokens(totals.outputTokens, locale)}>{compactTokens(totals.outputTokens)}</span></span>
        <span>{t('usage.metricCached')} <span className={styles.heroNum} title={exactTokens(totals.cachedTokens, locale)}>{compactTokens(totals.cachedTokens)}</span></span>
      </p>
    </div>
    <ul className={styles.derived}>
      {derived.map(card => <li key={card.text}>
        {/* 两层才等宽等高：外层 li 与内层卡片各撑一次（同 07 号规范的指标卡做法）。 */}
        <div className={styles.derivedCard}>
          <span className={styles.derivedValue} title={card.title}>{card.text}</span>
          {card.note && <span className={styles.derivedNote}>{card.note}</span>}
        </div>
      </li>)}
    </ul>
  </section>;
}

/**
 * 使用节奏：同一份逐日数据，热力图 / 柱状图 / 折线图三选一。
 *
 * 三张图的口径完全相同（当日总 Token），换的只是画法：热力图看「哪几天在用、断在哪」，
 * 柱状图量「某天到底多少」，折线图看走势与峰值。选哪一种由用户在卡片头部切换，
 * 悬停任意一天都会在读数那行给出「日期 · 用量 · 会话数」，三种画法的读法一致。
 */
function Rhythm({ report, kind, kinds, onKindChange, locale }: {
  report: UsageReport;
  kind: DailyChartKind;
  kinds: Array<{ id: string; label: string }>;
  onKindChange: (id: string) => void;
  locale: string;
}) {
  // 可访问名称跟着画法走：读屏听到的必须是当前这一张，不能一律报「柱状图」。
  const ariaLabel = t(kind === 'heat' ? 'usage.heatAria' : kind === 'bar' ? 'usage.trendAria' : 'usage.lineAria');
  return <section className={styles.card}>
    <header className={styles.cardHeader}>
      <h2 className={styles.cardTitle}>{t('usage.trendTitle')}</h2>
      <div className={styles.chartControls}>
        {/* 一年档只有一种画法，不摆一个只剩一个选项的分段控件（那看着像坏了）。 */}
        {kinds.length > 1 && <SegmentedTabs
          tabs={kinds}
          active={kind}
          onChange={onKindChange}
          ariaLabel={t('usage.trendTitle')}
        />}
        <span className={styles.cardMeta}>{t('usage.trendSessions', { count: report.sessions })}</span>
      </div>
    </header>
    <UsageTrendChart
      kind={kind}
      days={report.daily}
      ariaLabel={ariaLabel}
      peakLabel={t('usage.trendPeak', { value: compactTokens(peakTotal(report.daily)) })}
      locale={locale}
    />
  </section>;
}

/**
 * Token 构成。
 *
 * **不画一根把输入与输出接起来的堆叠条**：输出通常只有输入的百分之一到百分之二（真机 ~2%），
 * 接在同一根条上它是几个像素宽的一条线，读不出任何东西；而且那根条会让人以为「两段相加＝总量」。
 * 改成两行、每行按自身归一化：整条 = 这一项自己的总量，深色子段 = 它内部被单独标出的那部分
 * （输入里的缓存命中、输出里的推理），所以两行之间没有可加性，也就不存在加总暗示。
 *
 * 子段的数字与百分比都写在条外（`--font-mono` 在卡片底上读），条本身不承载文字：
 * 条太细，压在色块上的小字对比度不够，也读不准。条是装饰（`aria-hidden`），
 * 事实由旁边的文字与 `usage.compositionNote` 承担。
 */
function Composition({ totals, locale }: { totals: UsageTotals; locale: string }) {
  return <section className={styles.card}>
    <h2 className={styles.cardTitle}>{t('usage.compositionTitle')}</h2>
    <div className={styles.compose}>
      <ComposeRow
        label={t('usage.colInput')}
        part={t('usage.colCached')}
        partValue={totals.cachedTokens}
        note={t('usage.metricCachedNote')}
        percent={sharePercent(totals.cachedTokens, totals.inputTokens)}
        locale={locale}
      />
      <ComposeRow
        label={t('usage.colOutput')}
        part={t('usage.compositionReasoning')}
        partValue={totals.reasoningTokens}
        percent={sharePercent(totals.reasoningTokens, totals.outputTokens)}
        locale={locale}
      />
    </div>
    <p className={styles.composeNote}>{t('usage.compositionNote')}</p>
  </section>;
}

/** 构成条的一行：整条 = 这一项自己的总量，深色子段 = 它内部的缓存命中 / 推理。 */
function ComposeRow({ label, part, partValue, percent, note, locale }: {
  label: string;
  part?: string;
  partValue: number;
  percent: number;
  note?: string;
  locale: string;
}) {
  const width = clampPercent(percent);
  return <div className={styles.composeRow}>
    <span className={styles.composeLabel}>{label}</span>
    <span className={styles.composeTrack} aria-hidden="true">
      <span className={styles.composeRest} />
      <span className={styles.composePart} style={{ width: `${width}%` }} />
    </span>
    <span className={styles.composeReadout}>
      {part && <span className={styles.composePartName}>{part}</span>}
      <span className={styles.composeValue} title={exactTokens(partValue, locale)}>{compactTokens(partValue)}</span>
      <span className={styles.composePercent}>{t('usage.rankShare', { percent: percentLabel(percent) })}</span>
      {note && <span className={styles.composeNoteInline}>{note}</span>}
    </span>
  </div>;
}

/**
 * 按模型 / 按供应商的排行。每行 = 名称 + 占总量的比例条 + 合计 + 会话数。
 *
 * 比例条与百分比都以**本范围总量**为分母（`sharePercent`）：两张卡横着能比，
 * 也能直接对上结论带里的总 Token。分母写的是总量而不是本表首行——首行当分母只能看出相对名次，
 * 看不出「这家供应商占了全部用量的多少」。
 */
function Ranking({ title, firstColumn, rows, whole, locale, mono }: {
  title: string;
  firstColumn: string;
  rows: Array<{ key: string; sessions: number; totals: UsageTotals }>;
  whole: number;
  locale: string;
  mono?: boolean;
}) {
  return <section className={styles.card}>
    <h2 className={styles.cardTitle}>{title}</h2>
    {rows.length === 0
      ? <p className={styles.note}>{t('usage.tableEmpty')}</p>
      : <div className={styles.tableScroll}>
        <table className={styles.table}>
          <thead>
            <tr>
              <th scope="col">{firstColumn}</th>
              <th scope="col" className={styles.numHead}>{t('usage.colSessions')}</th>
              <th scope="col" className={styles.numHead}>{t('usage.colTotal')}</th>
            </tr>
          </thead>
          <tbody>
            {rows.map(row => {
              const percent = sharePercent(row.totals.totalTokens, whole);
              return <tr key={row.key}>
                <td>
                  <div className={styles.rankName}>
                    {/* 名称与供应商名是标识符：走等宽栈，长 id 也不会挤成两行。 */}
                    <span className={mono ? styles.mono : undefined}>{row.key}</span>
                    <span className={styles.rankBar}>
                      <span className={styles.rankTrack}>
                        <span className={styles.rankFill} style={{ width: `${clampPercent(percent)}%` }} />
                      </span>
                      <span className={styles.rankShare}>{t('usage.rankShare', { percent: percentLabel(percent) })}</span>
                    </span>
                  </div>
                </td>
                <td className={styles.num}>{row.sessions}</td>
                <td className={`${styles.num} ${styles.numStrong}`} title={exactTokens(row.totals.totalTokens, locale)}>{compactTokens(row.totals.totalTokens)}</td>
              </tr>;
            })}
          </tbody>
        </table>
      </div>}
  </section>;
}

/** 本机计划额度。读不到时给一行说明，不给一个假的 0%。 */
function PlanWindow({ plan, locale }: { plan: UsageReport['planWindow']; locale: string }) {
  if (!plan) {
    return <section className={styles.card}>
      <h2 className={styles.cardTitle}>{t('usage.planTitle')}</h2>
      <p className={styles.note}>{t('usage.planNone')}</p>
    </section>;
  }
  const window = planWindowLabel(plan.windowMinutes);
  // 取整后同时喂给环与那行文字：环按 35% 画，文字写「已用 35%」，两者不会差一档。
  const percent = percentLabel(plan.usedPercent);
  const used = t('usage.planUsed', { percent });
  return <section className={styles.card}>
    <h2 className={styles.cardTitle}>{t('usage.planTitle')}</h2>
    <div className={styles.planRow}>
      <PlanRing percent={Number(percent)} label={used} />
      <ul className={styles.planFacts}>
        <li>{t('usage.planType', { plan: plan.planType })}</li>
        <li>{t(window.key, window.vars)}</li>
        <li>{t('usage.planResets', { when: localDateTime(plan.resetsAt, locale) })}</li>
      </ul>
    </div>
  </section>;
}

const RING_RADIUS = 50;
const RING_CIRCUMFERENCE = 2 * Math.PI * RING_RADIUS;

/**
 * 环形进度。百分比既在环里（大字）也在环外的文字里（`已用 {percent}%`）——
 * 进度条按目测读不准，数字必须能直接读。
 *
 * 环形用 SVG 而不是 `conic-gradient`：圆头端点要靠 `stroke-linecap: round`，
 * 而且 0% 时能干脆地不画那一段（否则线帽会留下一个看着像 1% 的点）。
 * 图形本身 `aria-hidden`：事实由环外那行文字承担，不必让读屏把同一个数念两遍。
 */
function PlanRing({ percent, label }: { percent: number; label: string }) {
  const offset = RING_CIRCUMFERENCE * (1 - Math.min(100, Math.max(0, percent)) / 100);
  return <div className={styles.ring}>
    <svg className={styles.ringSvg} viewBox="0 0 112 112" aria-hidden="true" focusable="false">
      <circle className={styles.ringTrack} cx="56" cy="56" r={RING_RADIUS} />
      {percent > 0 && <circle
        className={styles.ringFill}
        cx="56" cy="56" r={RING_RADIUS}
        transform="rotate(-90 56 56)"
        strokeDasharray={RING_CIRCUMFERENCE}
        strokeDashoffset={offset}
      />}
    </svg>
    <span className={styles.ringValue}>{label}</span>
  </div>;
}
