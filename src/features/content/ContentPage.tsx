import { useCallback, useEffect, useMemo, useState } from 'react';
import { ExternalLink, Newspaper, Plus, RefreshCw, Rss, Trash2, TriangleAlert } from 'lucide-react';
import type { ContentStatus, FeedItem, FeedSource, FeedSourceDraft, RefreshReport } from '@/contracts/types';
import { type DesktopClient, toCoreError } from '@/desktop/client';
import { EmptyState } from '@/components/EmptyState';
import { SegmentedTabs } from '@/components/SegmentedTabs';
import { showToast } from '@/components/Toast';
import { t, useLocale } from '@/i18n';
import styles from './ContentPage.module.css';

/** 一页装载多少条；「加载更早」从本地库里翻，不重新联网。 */
const PAGE_SIZE = 100;

const GITHUB_WINDOWS = [
  { key: 'github-today', label: 'content.window.today' },
  { key: 'github-week', label: 'content.window.week' },
  { key: 'github-month', label: 'content.window.month' },
] as const;

function relative(seconds: number, locale: string): string {
  const formatter = new Intl.RelativeTimeFormat(locale, { numeric: 'auto' });
  const value = Math.max(0, seconds);
  if (value < 60) return formatter.format(-Math.round(value), 'second');
  if (value < 3600) return formatter.format(-Math.round(value / 60), 'minute');
  if (value < 86_400) return formatter.format(-Math.round(value / 3600), 'hour');
  return formatter.format(-Math.round(value / 86_400), 'day');
}

/**
 * 内容中心（设计 P-C1）。
 *
 * 与参考产品的根本差别：**没有云端聚合**，本机直接抓公开源。因此这一页的重点是
 * 「说清楚这些内容是怎么来的、什么时候取到的、哪些源现在取不到」：
 *
 * - 状态行永远显示「上次更新 / 下次更新」，抓取失败时保留旧内容并转琥珀；
 * - 订阅源页列出每个源的上次成功时间与连续失败次数；
 * - 页面明说抓取只在应用运行时发生。
 */
export function ContentPage({ client }: { client: DesktopClient }) {
  const locale = useLocale();
  const [tab, setTab] = useState<'news' | 'github' | 'sources'>('news');
  const [items, setItems] = useState<FeedItem[]>([]);
  const [sources, setSources] = useState<FeedSource[]>([]);
  const [status, setStatus] = useState<ContentStatus | null>(null);
  const [loading, setLoading] = useState(true);
  const [refreshing, setRefreshing] = useState(false);
  const [error, setError] = useState('');
  const [offset, setOffset] = useState(0);
  const [hasMore, setHasMore] = useState(false);
  const [window_, setWindow] = useState<string>('github-week');
  const [report, setReport] = useState<RefreshReport | null>(null);

  const [draftLabel, setDraftLabel] = useState('');
  const [draftUrl, setDraftUrl] = useState('');
  const [draftKind, setDraftKind] = useState<FeedSourceDraft['kind']>('rss');
  const [tokenConfigured, setTokenConfigured] = useState(false);

  const load = useCallback(async () => {
    try {
      const [nextItems, nextSources, nextStatus] = await Promise.all([
        client.listFeedItems({ limit: PAGE_SIZE, offset: 0 }),
        client.listFeedSources(),
        client.contentStatus(),
      ]);
      setItems(nextItems);
      setHasMore(nextItems.length === PAGE_SIZE);
      setOffset(nextItems.length);
      setSources(nextSources);
      setStatus(nextStatus);
      setError('');
    } catch (cause) {
      const core = toCoreError(cause);
      setError(core.safeDetails[0] ?? t(core.messageKey));
    } finally {
      setLoading(false);
    }
  }, [client]);

  useEffect(() => { void load(); }, [load]);
  useEffect(() => {
    void client.contentGithubTokenStatus().then(setTokenConfigured).catch(() => setTokenConfigured(false));
  }, [client]);

  const loadMore = async () => {
    try {
      const more = await client.listFeedItems({ limit: PAGE_SIZE, offset });
      setItems(current => [...current, ...more]);
      setOffset(current => current + more.length);
      setHasMore(more.length === PAGE_SIZE);
    } catch (cause) {
      const core = toCoreError(cause);
      showToast(core.safeDetails[0] ?? t(core.messageKey));
    }
  };

  const refresh = async (sourceId?: string) => {
    setRefreshing(true);
    try {
      const result = await client.refreshContent({ sourceId, force: true });
      setReport(result);
      await load();
      if (result.failed.length) {
        showToast(t('content.refresh.partial', { failed: result.failed.length, attempted: result.attempted.length }));
      } else {
        showToast(t('content.refresh.done', { count: result.newItems }));
      }
    } catch (cause) {
      const core = toCoreError(cause);
      showToast(core.safeDetails[0] ?? t(core.messageKey));
    } finally {
      setRefreshing(false);
    }
  };

  const now = Math.floor(Date.now() / 1000);
  const news = useMemo(
    () => items.filter(item => !item.sourceId.startsWith('github-')),
    [items],
  );
  const github = useMemo(
    () => items.filter(item => item.sourceId === window_),
    [items, window_],
  );
  const failing = status?.failing ?? [];

  const saveSource = async () => {
    if (!draftLabel.trim() || !draftUrl.trim()) return;
    try {
      await client.saveFeedSource({
        kind: draftKind,
        url: draftUrl.trim(),
        label: draftLabel.trim(),
        lang: draftKind === 'rss' ? (locale === 'zh-CN' ? 'zh' : 'en') : '',
        enabled: true,
      });
      setDraftLabel('');
      setDraftUrl('');
      await load();
      showToast(t('content.sources.added'));
    } catch (cause) {
      const core = toCoreError(cause);
      showToast(core.safeDetails[0] ?? t(core.messageKey));
    }
  };

  const toggleSource = async (source: FeedSource) => {
    try {
      await client.saveFeedSource({
        id: source.id, kind: source.kind, url: source.url, label: source.label,
        lang: source.lang, enabled: !source.enabled,
      });
      await load();
    } catch (cause) {
      const core = toCoreError(cause);
      showToast(core.safeDetails[0] ?? t(core.messageKey));
    }
  };

  const deleteSource = async (source: FeedSource) => {
    try {
      await client.deleteFeedSource(source.id);
      await load();
      showToast(t('content.sources.removed', { label: source.label }));
    } catch (cause) {
      const core = toCoreError(cause);
      showToast(core.safeDetails[0] ?? t(core.messageKey));
    }
  };

  const setToken = async () => {
    const value = window.prompt(t('content.token.prompt'));
    if (value === null) return;
    try {
      const configured = await client.setContentGithubToken(value.trim() || null);
      setTokenConfigured(configured);
      showToast(configured ? t('content.token.saved') : t('content.token.cleared'));
    } catch (cause) {
      const core = toCoreError(cause);
      showToast(core.safeDetails[0] ?? t(core.messageKey));
    }
  };

  const statusLine = () => {
    if (!status) return t('content.status.unknown');
    const parts: string[] = [];
    parts.push(status.lastOkAt
      ? t('content.status.lastOk', { when: relative(now - status.lastOkAt, locale) })
      : t('content.status.neverOk'));
    parts.push(status.nextFetchAt > now
      ? t('content.status.next', { when: relative(status.nextFetchAt - now, locale) })
      : t('content.status.due'));
    parts.push(t('content.status.total', { count: status.totalItems }));
    return parts.join(' · ');
  };

  return (
    <div className={styles.page}>
      <SegmentedTabs
        ariaLabel={t('content.title')}
        active={tab}
        onChange={id => setTab(id as 'news' | 'github' | 'sources')}
        tabs={[
          { id: 'news', label: t('content.tab.news') },
          { id: 'github', label: t('content.tab.github') },
          { id: 'sources', label: t('content.tab.sources') },
        ]} />

      {/* 状态行：这一页最重要的信息，永远可见。 */}
      {tab !== 'sources' && (
        <section className={styles.statusBar}>
          <span className={failing.length ? styles.statusWarn : styles.statusOk}>
            {failing.length > 0 && <TriangleAlert size={13} />}
            {statusLine()}
          </span>
          <button type="button" onClick={() => void refresh()} disabled={refreshing}>
            <RefreshCw size={15} className={refreshing ? styles.spin : ''} />
            {refreshing ? t('content.refreshing') : t('content.refreshNow')}
          </button>
        </section>
      )}

      {failing.length > 0 && tab !== 'sources' && (
        <ul className={styles.failureList} role="status">
          {failing.map(failure => (
            <li key={failure.sourceId}>
              <TriangleAlert size={13} />
              {t('content.failure.line', {
                label: failure.label, streak: failure.failStreak, message: failure.message,
              })}
            </li>
          ))}
          <li className={styles.failureHint}>{t('content.failure.kept')}</li>
        </ul>
      )}

      {error && <div className="error-message" role="alert">{error}</div>}

      {report && (
        <section className={styles.report} role="status" aria-live="polite">
          <strong>{t('content.refresh.reportTitle')}</strong>
          <span>{t('content.refresh.reportCounts', {
            succeeded: report.succeeded.length,
            unchanged: report.notModified.length,
            failed: report.failed.length,
            skipped: report.skipped.length,
            added: report.newItems,
          })}</span>
          {report.skipped.length > 0 && <span className={styles.statusWarn}>{t('content.refresh.budget')}</span>}
          <button type="button" onClick={() => setReport(null)}>{t('common.close')}</button>
        </section>
      )}

      {loading ? (
        <div className={styles.empty} role="status" aria-live="polite">{t('content.loading')}</div>
      ) : tab === 'sources' ? (
        <>
          <section className={styles.card}>
            <div className={styles.header}>
              <h2>{t('content.sources.title')}</h2>
              {/* 抓取时刻不再让用户挑：固定每天两次（核心的 DAILY_FETCH_HOURS）。
                  小时数由状态带回，界面不自己写一份，改规则时不会两边不一致。 */}
              <span className={styles.schedule}>{t('content.sources.schedule', {
                // 时刻是一个列表，用顿号/逗号并列，不用分句用的那个分隔符。
                hours: (status?.scheduleHours ?? [6, 18]).map(hour => `${String(hour).padStart(2, '0')}:00`).join(t('common.itemSeparator')),
              })}</span>
            </div>
            <p className={styles.note}>{t('content.sources.runtimeNote')}</p>
            <ul className={styles.sourceList}>
              {sources.map(source => (
                <li key={source.id}>
                  <div className={styles.sourceRow}>
                    <div className={styles.sourceMain}>
                      <span className={styles.sourceLabel}>
                        {source.label}
                        {source.builtin && <span className={styles.builtinTag}>{t('content.sources.builtin')}</span>}
                        {!source.enabled && <span className="badge">{t('content.sources.disabled')}</span>}
                      </span>
                      <span className={`${styles.sourceUrl} text-mono break-anywhere`}>{source.url}</span>
                      <span className={styles.sourceMeta}>
                        {source.lastOkAt
                          ? t('content.sources.lastOk', { when: relative(now - source.lastOkAt, locale) })
                          : t('content.sources.neverOk')}
                        {source.failStreak > 0 && ` · ${t('content.sources.streak', { streak: source.failStreak })}`}
                        {source.lastError && ` · ${source.lastError}`}
                      </span>
                    </div>
                    <div className={styles.sourceActions}>
                      <label className="check-label">
                        <input type="checkbox" checked={source.enabled} onChange={() => void toggleSource(source)}
                          aria-label={t('content.sources.enable', { label: source.label })} />
                        <span>{source.enabled ? t('content.sources.enabled') : t('content.sources.disabled')}</span>
                      </label>
                      <button type="button" onClick={() => void refresh(source.id)} disabled={refreshing}>
                        <RefreshCw size={15} />{t('content.sources.refreshOne')}
                      </button>
                      <button type="button" onClick={() => void deleteSource(source)}>
                        <Trash2 size={15} />{t('action.delete')}
                      </button>
                    </div>
                  </div>
                </li>
              ))}
            </ul>
          </section>

          <section className={styles.card}>
            <h2>{t('content.sources.addTitle')}</h2>
            <div className={styles.addRow}>
              <label>
                <span>{t('content.sources.kind')}</span>
                <select value={draftKind} onChange={event => setDraftKind(event.target.value as FeedSourceDraft['kind'])}>
                  <option value="rss">{t('content.sources.kind.rss')}</option>
                  <option value="githubSearch">{t('content.sources.kind.github')}</option>
                </select>
              </label>
              <label className={styles.grow}>
                <span>{t('content.sources.name')}</span>
                <input value={draftLabel} onChange={event => setDraftLabel(event.target.value)}
                  placeholder={t('content.sources.namePlaceholder')} />
              </label>
              <label className={styles.grow}>
                <span>{t('content.sources.url')}</span>
                {draftKind === 'rss' ? (
                  <input value={draftUrl} onChange={event => setDraftUrl(event.target.value)}
                    placeholder="https://example.com/feed" className="text-mono" />
                ) : (
                  <select value={draftUrl} onChange={event => setDraftUrl(event.target.value)}>
                    <option value="today">today</option>
                    <option value="week">week</option>
                    <option value="month">month</option>
                  </select>
                )}
              </label>
              <button className="primary" type="button" onClick={() => void saveSource()}
                disabled={!draftLabel.trim() || !draftUrl.trim()}>
                <Plus size={15} />{t('content.sources.add')}
              </button>
            </div>
            <p className={styles.note}>{t('content.sources.githubNote')}</p>
          </section>

          <section className={styles.card}>
            <div className={styles.header}>
              <h2>{t('content.token.title')}</h2>
              <span className="badge">{tokenConfigured ? t('content.token.configured') : t('content.token.absent')}</span>
            </div>
            <p className={styles.note}>{t('content.token.body')}</p>
            <div className="actions">
              <button type="button" onClick={() => void setToken()}>
                {tokenConfigured ? t('content.token.update') : t('content.token.set')}
              </button>
              {tokenConfigured && (
                <button type="button" onClick={() => void client.setContentGithubToken(null).then(() => setTokenConfigured(false))}>
                  {t('content.token.clear')}
                </button>
              )}
            </div>
          </section>
        </>
      ) : tab === 'news' ? (
        news.length ? (
          <section className={styles.card}>
            <ul className={styles.itemList}>
              {news.map(item => (
                <li key={item.url}>
                  <button type="button" className={styles.itemRow} onClick={() => window.open(item.url, '_blank', 'noreferrer')}>
                    <span className={styles.itemTitle}>{item.title}</span>
                    <span className={styles.itemMeta}>
                      {item.sourceLabel} · {relative(now - item.publishedAt, locale)}
                      <ExternalLink size={11} />
                    </span>
                  </button>
                </li>
              ))}
            </ul>
            {hasMore && <button type="button" className={styles.more} onClick={() => void loadMore()}>{t('content.loadMore')}</button>}
          </section>
        ) : (
          <EmptyState icon={Newspaper} title={t('content.news.empty.title')} description={t('content.news.empty.body')}
            action={<button type="button" onClick={() => void refresh()} disabled={refreshing}>{t('content.refreshNow')}</button>} />
        )
      ) : (
        <>
          <section className={styles.statusBar}>
            {/* 时间窗口也是互斥视图切换，用与页签同一个分段控件，不再各画一套。 */}
            <SegmentedTabs
              ariaLabel={t('content.tab.github')}
              active={window_}
              onChange={setWindow}
              tabs={GITHUB_WINDOWS.map(option => ({ id: option.key, label: t(option.label) }))} />
            <span className={styles.statusOk}>{t('content.github.source')}</span>
          </section>
          {github.length ? (
            <section className={styles.card}>
              <ul className={styles.repoGrid}>
                {github.map(item => (
                  <li key={item.url}>
                    <button type="button" className={styles.repoCard} onClick={() => window.open(item.url, '_blank', 'noreferrer')}>
                      <span className={styles.repoName}>{item.repo ?? item.title}</span>
                      <span className={styles.repoStars}>★ {item.stars ?? '—'}</span>
                      <span className={styles.repoSummary}>{item.summary}</span>
                    </button>
                  </li>
                ))}
              </ul>
            </section>
          ) : (
            <EmptyState icon={Rss} title={t('content.github.empty.title')} description={t('content.github.empty.body')}
              action={<button type="button" onClick={() => void refresh('github-today')} disabled={refreshing}>{t('content.refreshNow')}</button>} />
          )}
        </>
      )}
    </div>
  );
}
