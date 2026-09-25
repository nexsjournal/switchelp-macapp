import { useCallback, useEffect, useState } from 'react';
import { ChartColumn, RefreshCw } from 'lucide-react';
import type { UsageReport, UsageTotals } from '@/contracts/types';
import { type DesktopClient, toCoreError } from '@/desktop/client';
import { EmptyState } from '@/components/EmptyState';
import { SegmentedTabs } from '@/components/SegmentedTabs';
import { showToast } from '@/components/Toast';
import { t, useLocale } from '@/i18n';
import { UsageTrendChart } from './UsageTrendChart';
import { compactTokens, exactTokens, localDateTime, peakTotal, planWindowLabel, usedPercentLabel } from './usagePolicy';
import styles from './UsagePage.module.css';

/**
 * 用量页。只读本机 Codex 会话记录，统计 Token 用量与本机计划额度窗口。
 *
 * 三件刻意不做的事（理由见 docs/design/07-usage-page.md）：
 * - **不显示成本**：本工具的模型是用户自定义的第三方模型，没有可靠的单价来源。
 *   按规范「不展示没有可靠来源的数字」，宁可只给 token 真值。
 * - **四个指标不可相加**：`缓存读取 ⊂ 输入`，且真实数据里 `total ≠ 输入 + 输出`
 *   （约 9% 的事件有 ±1~7 的差），所以卡片上是四个并列口径，并写明包含关系。
 * - **计划额度没有就是没有**：一条 `rate_limits` 都读不到时不显示这张卡（也不显示 0%），
 *   换成一行说明——只有官方计划账号的会话才记录这个窗口。
 */

const RANGES = [
  { id: '7', label: 'usage.range7' },
  { id: '30', label: 'usage.range30' },
  { id: '90', label: 'usage.range90' },
] as const;

type RangeId = (typeof RANGES)[number]['id'];

const RANGE_DAYS: Record<RangeId, number> = { '7': 7, '30': 30, '90': 90 };

export function UsagePage({ client }: { client: DesktopClient }) {
  const locale = useLocale();
  const [range, setRange] = useState<RangeId>('30');
  const [report, setReport] = useState<UsageReport | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState('');

  const load = useCallback(async (next: RangeId) => {
    setLoading(true);
    try {
      const result = await client.usageReport(RANGE_DAYS[next]);
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
          <Metrics totals={report.totals} locale={locale} />

          {report.sessions === 0 && <div className={styles.note}>{t('usage.rangeEmpty', { days: report.rangeDays })}</div>}

          <section className={styles.card}>
            <header className={styles.cardHeader}>
              <h2 className={styles.cardTitle}>{t('usage.trendTitle')}</h2>
              <span className={styles.cardMeta}>{t('usage.trendSessions', { count: report.sessions })}</span>
            </header>
            <UsageTrendChart
              days={report.daily}
              ariaLabel={t('usage.trendAria')}
              peakLabel={t('usage.trendPeak', { value: compactTokens(peakTotal(report.daily)) })}
              locale={locale}
            />
          </section>

          <PlanWindow plan={report.planWindow} locale={locale} />

          <Breakdown
            title={t('usage.modelTitle')}
            firstColumn={t('usage.colModel')}
            rows={report.byModel.map(row => ({ key: row.model, sessions: row.sessions, totals: row.totals }))}
            locale={locale}
            mono
          />
          <Breakdown
            title={t('usage.providerTitle')}
            firstColumn={t('usage.colProvider')}
            rows={report.byProvider.map(row => ({ key: row.provider, sessions: row.sessions, totals: row.totals }))}
            locale={locale}
          />

          {/* 证据行：数字来自哪个目录、扫了多少文件、有多少读不了，都如实写出来。
              有不可读的文件时不能只字不提——那会让「合计」看起来比实际更完整。 */}
          <p className={styles.sourceLine}>
            {t('usage.sourceLine', { directory: report.sourceDirectory, files: report.scannedFiles })}
            {report.unreadableFiles > 0 && <> · <span className={styles.sourceWarn}>{t('usage.unreadable', { count: report.unreadableFiles })}</span></>}
          </p>
        </>}
  </div>;
}

/** 四个指标卡。同一单位、并列口径；缓存读取卡写明它含在输入内。 */
function Metrics({ totals, locale }: { totals: UsageTotals; locale: string }) {
  const cards = [
    { label: 'usage.metricTotal', value: totals.totalTokens },
    { label: 'usage.metricInput', value: totals.inputTokens },
    { label: 'usage.metricOutput', value: totals.outputTokens },
    { label: 'usage.metricCached', value: totals.cachedTokens, note: 'usage.metricCachedNote' },
  ];
  return <ul className={styles.metrics}>
    {cards.map(card => <li key={card.label} className={styles.metricCell}>
      {/* 内层元素负责撑满：grid 只拉伸 li，卡片本身要靠 flex:1 才等宽等高。 */}
      <div className={styles.metricCard}>
        <span className={styles.metricLabel}>{t(card.label)}</span>
        <span className={styles.metricValue} title={exactTokens(card.value, locale)}>{compactTokens(card.value)}</span>
        {card.note && <span className={styles.metricNote}>{t(card.note)}</span>}
      </div>
    </li>)}
  </ul>;
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
  const percent = usedPercentLabel(plan.usedPercent);
  return <section className={styles.card}>
    <header className={styles.cardHeader}>
      <h2 className={styles.cardTitle}>{t('usage.planTitle')}</h2>
      <span className={styles.cardMeta}>
        {t('usage.planType', { plan: plan.planType })} · {t(window.key, window.vars)}
      </span>
    </header>
    <div className={styles.planRow}>
      {/* 进度条按真实百分比取值；数字另用等宽字体写出来，不靠目测读进度条。 */}
      <div className={styles.planTrack} role="img" aria-label={`${t('usage.planUsed', { percent })}`}>
        <div className={styles.planFill} style={{ width: `${percent}%` }} />
      </div>
      <span className={styles.planValue}>{t('usage.planUsed', { percent })}</span>
      <span className={styles.planReset}>{t('usage.planResets', { when: localDateTime(plan.resetsAt, locale) })}</span>
    </div>
  </section>;
}

/** 按模型 / 按供应商的表格。表头必须带 scope="col"（审计脚本会查）。 */
function Breakdown({ title, firstColumn, rows, locale, mono }: {
  title: string;
  firstColumn: string;
  rows: Array<{ key: string; sessions: number; totals: UsageTotals }>;
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
              <th scope="col" className={styles.numHead}>{t('usage.colInput')}</th>
              <th scope="col" className={styles.numHead}>{t('usage.colOutput')}</th>
              <th scope="col" className={styles.numHead}>{t('usage.colCached')}</th>
              <th scope="col" className={styles.numHead}>{t('usage.colTotal')}</th>
            </tr>
          </thead>
          <tbody>
            {rows.map(row => <tr key={row.key}>
              <td className={mono ? styles.mono : undefined}>{row.key}</td>
              <td className={styles.num}>{row.sessions}</td>
              <td className={styles.num} title={exactTokens(row.totals.inputTokens, locale)}>{compactTokens(row.totals.inputTokens)}</td>
              <td className={styles.num} title={exactTokens(row.totals.outputTokens, locale)}>{compactTokens(row.totals.outputTokens)}</td>
              <td className={styles.num} title={exactTokens(row.totals.cachedTokens, locale)}>{compactTokens(row.totals.cachedTokens)}</td>
              <td className={`${styles.num} ${styles.numStrong}`} title={exactTokens(row.totals.totalTokens, locale)}>{compactTokens(row.totals.totalTokens)}</td>
            </tr>)}
          </tbody>
        </table>
      </div>}
  </section>;
}
