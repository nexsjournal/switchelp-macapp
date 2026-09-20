import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { AlertTriangle, Download, Info, RefreshCw, Save, ScrollText, ShieldCheck, Trash2, XCircle } from 'lucide-react';
import type { DiagnosticEvent, LogLevel } from '@/contracts/types';
import { type DesktopClient, type DiagnosticsPreview, toCoreError } from '@/desktop/client';
import { Dialog } from '@/components/Dialog';
import { EmptyState } from '@/components/EmptyState';
import styles from './LogsPage.module.css';

import { t } from '@/i18n';
/**
 * 日志页（设计 P08）。
 *
 * 三件事分开：**看**（筛选 + 分页 + 详情）、**带走**（诊断包预览后保存）、
 * **清**（只清本工具自己的记录，不碰 Codex 历史）。
 * 全文搜索只在已脱敏字段里进行——日志里本来也没有正文。
 */

const PAGE_SIZE = 200;

const levelKeys: Record<LogLevel, string> = { info: 'logs.levelInfo', warning: 'logs.levelWarning', error: 'logs.levelError' };
const levelIcons = { info: Info, warning: AlertTriangle, error: XCircle } as const;

/** 类别与导出范围保持同一套前缀，避免两处各写一份。 */
const CATEGORIES = ['gateway', 'apply', 'probe', 'discovery', 'credential'] as const;

type TimeRange = 'all' | '15m' | '1h' | '24h';

const timeKeys: Record<TimeRange, string> = { all: 'logs.timeAll', '15m': 'logs.time15m', '1h': 'logs.time1h', '24h': 'logs.time24h' };

const rangeSeconds: Record<Exclude<TimeRange, 'all'>, number> = { '15m': 900, '1h': 3600, '24h': 86_400 };

export function LogsPage({ client }: { client: DesktopClient }) {
  const [events, setEvents] = useState<DiagnosticEvent[]>([]);
  const [level, setLevel] = useState<LogLevel | 'all'>('all');
  const [category, setCategory] = useState<string>('all');
  const [range, setRange] = useState<TimeRange>('all');
  const [query, setQuery] = useState('');
  const [page, setPage] = useState(0);
  const [detail, setDetail] = useState<DiagnosticEvent | null>(null);
  const [selectedScopes, setSelectedScopes] = useState<string[]>([...CATEGORIES]);
  const [preview, setPreview] = useState<DiagnosticsPreview | null>(null);
  const [savedPath, setSavedPath] = useState('');
  const [busy, setBusy] = useState('');
  const [error, setError] = useState('');
  const [notice, setNotice] = useState('');
  const [confirmClear, setConfirmClear] = useState(false);

  /** 加载序号：连续切换筛选时，先返回的旧响应不得覆盖后发起的查询。 */
  const pendingLoad = useRef(0);

  const load = useCallback(async () => {
    const ticket = ++pendingLoad.current;
    setBusy('load'); setError('');
    try {
      const result = await client.listDiagnostics(level === 'all' ? {} : { level });
      if (ticket !== pendingLoad.current) return;
      setEvents(result.items);
      setPage(0);
    } catch (thrown) {
      if (ticket !== pendingLoad.current) return;
      setError(toCoreError(thrown).safeDetails.join(t('common.listSeparator')) || t('logs.loadFailed'));
    }
    finally { if (ticket === pendingLoad.current) setBusy(''); }
  }, [client, level]);

  useEffect(() => { void load(); }, [load]);

  async function run(label: string, work: () => Promise<void>) {
    setBusy(label); setError(''); setNotice('');
    try { await work(); }
    catch (thrown) { setError(toCoreError(thrown).safeDetails.join(t('common.listSeparator')) || t('common.failed')); }
    finally { setBusy(''); }
  }

  /** 时间与全文筛选在前端做：只影响展示，不改变后端保留策略。 */
  const filtered = useMemo(() => {
    const text = query.trim().toLocaleLowerCase();
    const cutoff = range === 'all' ? null : Date.now() - rangeSeconds[range] * 1000;
    return events.filter(event => {
      if (category !== 'all' && !event.categoryKey.startsWith(category)) return false;
      if (cutoff !== null) {
        const stamp = Date.parse(event.timestamp);
        if (!Number.isNaN(stamp) && stamp < cutoff) return false;
      }
      if (!text) return true;
      const haystack = [event.categoryKey, event.targetLabel, event.resultKey, ...Object.values(event.safeMetadata ?? {})]
        .join(' ').toLocaleLowerCase();
      return haystack.includes(text);
    });
  }, [events, category, range, query]);

  const pageCount = Math.max(1, Math.ceil(filtered.length / PAGE_SIZE));
  const current = Math.min(page, pageCount - 1);
  const visible = useMemo(
    () => [...filtered].reverse().slice(current * PAGE_SIZE, current * PAGE_SIZE + PAGE_SIZE),
    [filtered, current],
  );

  /** 关联事件：同一目标或同一 operationId，便于把一次失败的前后文串起来。 */
  const related = useMemo(() => {
    if (!detail) return [];
    const operationId = detail.safeMetadata?.operation_id;
    return events.filter(event => event !== detail && (
      (operationId && event.safeMetadata?.operation_id === operationId) ||
      event.targetLabel === detail.targetLabel
    )).slice(-6).reverse();
  }, [detail, events]);

  const clear = () => run('clear', async () => {
    const removed = await client.clearDiagnostics();
    setConfirmClear(false);
    setDetail(null);
    setNotice(t('logs.cleared', { count: removed }));
    await load();
  });

  const makePreview = () => run('preview', async () => {
    setSavedPath('');
    setPreview(await client.previewDiagnostics({ scopes: selectedScopes, redactionPreviewHash: '' }));
  });

  const exportPackage = () => run('export', async () => {
    const result = await client.exportDiagnostics({ scopes: selectedScopes, redactionPreviewHash: '' });
    setSavedPath(result.savedPath);
  });

  return <div className={styles.page}>
    {error && <div className="error-message" role="alert">{error}</div>}
    {notice && <div className={styles.notice} role="status">{notice}</div>}

    <section className={styles.card}>
      <div className={styles.header}>
        <div><ScrollText size={17} /><h2>{t('logs.events')}</h2><span className="badge">{filtered.length}{filtered.length !== events.length ? ` / ${events.length}` : ''}</span></div>
        <div className={styles.actions}>
          <button onClick={() => void load()} disabled={busy === 'load'} className="icon-button" aria-label={t('logs.refresh')}>
            <RefreshCw size={17} className={busy === 'load' ? styles.spin : ''} />
          </button>
          <button className="danger" onClick={() => setConfirmClear(true)} disabled={!events.length}>
            <Trash2 size={16} />{t('logs.clear')}</button>
        </div>
      </div>

      <div className={styles.filters}>
        <label>{t('logs.level')}<select aria-label={t('logs.levelAria')} value={level} onChange={event => setLevel(event.target.value as LogLevel | 'all')}>
          <option value="all">{t('logs.levelAll')}</option>
          <option value="info">{t('logs.levelInfoUp')}</option>
          <option value="warning">{t('logs.levelWarningUp')}</option>
          <option value="error">{t('logs.levelErrorOnly')}</option>
        </select></label>
        <label>{t('logs.category')}<select aria-label={t('logs.categoryAria')} value={category} onChange={event => setCategory(event.target.value)}>
          <option value="all">{t('logs.levelAll')}</option>
          {CATEGORIES.map(item => <option key={item} value={item}>{item}</option>)}
        </select></label>
        <label>{t('logs.time')}<select aria-label={t('logs.timeAria')} value={range} onChange={event => setRange(event.target.value as TimeRange)}>
          {(Object.keys(timeKeys) as TimeRange[]).map(key => <option key={key} value={key}>{t(timeKeys[key])}</option>)}
        </select></label>
        <label className={styles.grow}>{t('common.search')}<input aria-label={t('logs.searchAria')} placeholder={t('logs.searchPlaceholder')}
          value={query} onChange={event => setQuery(event.target.value)} /></label>
      </div>

      <p className={styles.policy}>{t('logs.policy')}</p>

      {visible.length === 0
        ? <EmptyState icon={ScrollText} title={events.length ? t('logs.noMatch') : t('logs.empty')}
            description={events.length ? t('logs.noMatchBody') : t('logs.emptyBody')} />
        : <ul className={styles.events}>
          {visible.map((event, index) => {
            const Icon = levelIcons[event.level];
            return <li key={`${event.timestamp}-${event.resultKey}-${index}`}>
              <button className={styles.eventRow} onClick={() => setDetail(event)} aria-label={t('logs.detailAria', { result: event.resultKey })}>
                <Icon size={15} className={styles[event.level]} aria-hidden="true" />
                <span className="text-mono text-muted">{event.timestamp}</span>
                <span className={styles.category}>{event.categoryKey}</span>
                <span className="break-anywhere">{event.resultKey}</span>
                <span className="text-muted break-anywhere">{event.targetLabel}</span>
                <span className="text-mono text-muted">{event.elapsedMs != null ? `${event.elapsedMs} ms` : ''}</span>
              </button>
            </li>;
          })}
        </ul>}

      {pageCount > 1 && <div className={styles.pager}>
        <button onClick={() => setPage(current - 1)} disabled={current === 0}>{t('common.prevPage')}</button>
        <span>{t('logs.pager', { page: current + 1, pages: pageCount, size: PAGE_SIZE })}</span>
        <button onClick={() => setPage(current + 1)} disabled={current >= pageCount - 1}>{t('common.nextPage')}</button>
      </div>}
    </section>

    <section className={styles.card}>
      <div className={styles.header}>
        <div><h2>{t('logs.exportPackage')}</h2><ShieldCheck size={16} /></div>
        <div className={styles.actions}>
          <button onClick={() => void makePreview()} disabled={busy === 'preview'}>
            {busy === 'preview' ? t('logs.previewing') : t('logs.preview')}
          </button>
          <button className="primary" onClick={() => void exportPackage()} disabled={busy === 'export' || !preview}>
            <Save size={16} />{busy === 'export' ? t('editor.saving') : t('logs.saveLocally')}
          </button>
        </div>
      </div>

      <div className={styles.scopes}>
        {CATEGORIES.map(scope => <label key={scope} className="check-label">
          <input type="checkbox" checked={selectedScopes.includes(scope)}
            onChange={event => {
              setSelectedScopes(list => event.target.checked ? [...list, scope] : list.filter(item => item !== scope));
              // 范围变了，之前预览过的清单不再代表将要导出的内容：作废预览，
              // 否则「保存到本地」会导出与预览清单不一致的包。
              setPreview(null);
              setSavedPath('');
            }} />
          {scope}
        </label>)}
      </div>

      {preview && <div className={styles.preview}>
        <p className="text-muted">{t('logs.previewSummary', { bytes: preview.totalBytes.toLocaleString(), items: preview.items.length })}</p>
        <ul>{preview.items.map(item => <li key={item.name} className={item.included ? styles.included : ''}>
          <span>{item.name}</span>
          <span className={`badge ${item.included ? '' : 'warning'}`}>{item.included ? t('logs.included') : t('logs.excluded')}</span>
          <span className="text-muted break-anywhere">{item.note}</span>
        </li>)}</ul>
      </div>}

      {savedPath && <div className={styles.saved} role="status"><Download size={15} />{t('common.savedTo')}<span className="text-mono break-anywhere">{savedPath}</span></div>}
    </section>

    {detail && <Dialog title={t('logs.eventDetail')} description={`${detail.categoryKey} · ${t(levelKeys[detail.level])}`} onClose={() => setDetail(null)}>
      <div className="form-fields">
        <dl className={styles.detail}>
          <dt>{t('logs.time')}</dt><dd className="text-mono">{detail.timestamp}</dd>
          <dt>{t('logs.resultField')}</dt><dd>{detail.resultKey}</dd>
          <dt>{t('logs.targetField')}</dt><dd className="break-anywhere">{detail.targetLabel}</dd>
          <dt>{t('logs.elapsedField')}</dt><dd>{detail.elapsedMs != null ? `${detail.elapsedMs} ms` : '—'}</dd>
        </dl>
        <h3 className="form-section">{t('logs.safeMetadata')}</h3>
        {Object.keys(detail.safeMetadata ?? {}).length === 0
          ? <p className="text-muted">{t('logs.noMetadata')}</p>
          : <dl className={styles.detail}>{Object.entries(detail.safeMetadata).map(([key, value]) => <span key={key} className={styles.kv}>
            <dt className="text-mono">{key}</dt><dd className="text-mono break-anywhere">{String(value)}</dd>
          </span>)}</dl>}
        <h3 className="form-section">{t('logs.related')}<span>{t('logs.relatedScope')}</span></h3>
        {related.length === 0
          ? <p className="text-muted">{t('logs.noRelated')}</p>
          : <ul className={styles.related}>{related.map((event, index) => <li key={index}>
            <span className="text-mono text-muted">{event.timestamp}</span>
            <span className="break-anywhere">{event.resultKey}</span>
          </li>)}</ul>}
        <div className="actions" style={{ justifyContent: 'flex-end' }}>
          <button onClick={() => setDetail(null)} autoFocus>{t('common.close')}</button>
        </div>
      </div>
    </Dialog>}

    {confirmClear && <Dialog title={t('logs.clear')} busy={busy === 'clear'}
      description={t('logs.clearBody', { count: events.length })}
      onClose={() => setConfirmClear(false)}>
      <div className="form-fields"><div className="form-footer">
        <span>{t('logs.clearIrreversible')}</span>
        <div className="actions">
          <button onClick={() => setConfirmClear(false)} disabled={busy === 'clear'}>{t('action.cancel')}</button>
          <button className="danger" autoFocus disabled={busy === 'clear'} onClick={() => void clear()}>
            {busy === 'clear' ? t('logs.clearing') : t('logs.clear')}
          </button>
        </div>
      </div></div>
    </Dialog>}
  </div>;
}
