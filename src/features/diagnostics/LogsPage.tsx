import { useCallback, useEffect, useMemo, useState } from 'react';
import { AlertTriangle, Download, Info, RefreshCw, Save, ShieldCheck, Trash2, XCircle } from 'lucide-react';
import type { DiagnosticEvent, LogLevel } from '@/contracts/types';
import { type DesktopClient, type DiagnosticsPreview, toCoreError } from '@/desktop/client';
import { Dialog } from '@/components/Dialog';
import styles from './LogsPage.module.css';

/**
 * 日志页（设计 P08）。
 *
 * 三件事分开：**看**（筛选 + 分页 + 详情）、**带走**（诊断包预览后保存）、
 * **清**（只清本工具自己的记录，不碰 Codex 历史）。
 * 全文搜索只在已脱敏字段里进行——日志里本来也没有正文。
 */

const PAGE_SIZE = 200;

const levelLabels: Record<LogLevel, string> = { info: '信息', warning: '警告', error: '错误' };
const levelIcons = { info: Info, warning: AlertTriangle, error: XCircle } as const;

/** 类别与导出范围保持同一套前缀，避免两处各写一份。 */
const CATEGORIES = ['gateway', 'apply', 'probe', 'discovery', 'credential'] as const;

type TimeRange = 'all' | '15m' | '1h' | '24h';

const timeLabels: Record<TimeRange, string> = { all: '全部时间', '15m': '最近 15 分钟', '1h': '最近 1 小时', '24h': '最近 24 小时' };

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

  const load = useCallback(async () => {
    setBusy('load'); setError('');
    try {
      const result = await client.listDiagnostics(level === 'all' ? {} : { level });
      setEvents(result.items);
      setPage(0);
    } catch (thrown) { setError(toCoreError(thrown).safeDetails.join('；') || '日志加载失败。'); }
    finally { setBusy(''); }
  }, [client, level]);

  useEffect(() => { void load(); }, [load]);

  async function run(label: string, work: () => Promise<void>) {
    setBusy(label); setError(''); setNotice('');
    try { await work(); }
    catch (thrown) { setError(toCoreError(thrown).safeDetails.join('；') || '操作失败。'); }
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
    setNotice(`已清空 ${removed} 条本工具诊断事件；Codex 历史与配置事务记录未受影响。`);
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
        <div><h2>诊断事件</h2><span className="badge">{filtered.length}{filtered.length !== events.length ? ` / ${events.length}` : ''}</span></div>
        <div className={styles.actions}>
          <button onClick={() => void load()} disabled={busy === 'load'} className="icon-button" aria-label="刷新日志">
            <RefreshCw size={17} className={busy === 'load' ? styles.spin : ''} />
          </button>
          <button className="danger" onClick={() => setConfirmClear(true)} disabled={!events.length}>
            <Trash2 size={16} />清空日志
          </button>
        </div>
      </div>

      <div className={styles.filters}>
        <label>级别<select aria-label="日志级别" value={level} onChange={event => setLevel(event.target.value as LogLevel | 'all')}>
          <option value="all">全部</option>
          <option value="info">信息及以上</option>
          <option value="warning">警告及以上</option>
          <option value="error">仅错误</option>
        </select></label>
        <label>类别<select aria-label="日志类别" value={category} onChange={event => setCategory(event.target.value)}>
          <option value="all">全部</option>
          {CATEGORIES.map(item => <option key={item} value={item}>{item}</option>)}
        </select></label>
        <label>时间<select aria-label="时间范围" value={range} onChange={event => setRange(event.target.value as TimeRange)}>
          {(Object.keys(timeLabels) as TimeRange[]).map(key => <option key={key} value={key}>{timeLabels[key]}</option>)}
        </select></label>
        <label className={styles.grow}>搜索<input aria-label="搜索日志" placeholder="在脱敏字段中搜索：模型、供应商、结果、错误码"
          value={query} onChange={event => setQuery(event.target.value)} /></label>
      </div>

      <p className={styles.policy}>
        只记录时间、模型与供应商标识、阶段、HTTP 状态与耗时、错误码。prompt、completion、工具参数、
        文件内容、Authorization 与完整 URL query 从不写入，因此搜索也只在脱敏字段里进行。
      </p>

      {visible.length === 0
        ? <p className={styles.empty}>{events.length ? '当前筛选下没有事件。' : '还没有诊断事件。执行一次应用、测试连接或模型发现后，这里会出现脱敏后的记录。'}</p>
        : <ul className={styles.events}>
          {visible.map((event, index) => {
            const Icon = levelIcons[event.level];
            return <li key={`${event.timestamp}-${event.resultKey}-${index}`}>
              <button className={styles.eventRow} onClick={() => setDetail(event)} aria-label={`查看事件详情 ${event.resultKey}`}>
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
        <button onClick={() => setPage(current - 1)} disabled={current === 0}>上一页</button>
        <span>第 {current + 1} / {pageCount} 页 · 每页 {PAGE_SIZE} 条</span>
        <button onClick={() => setPage(current + 1)} disabled={current >= pageCount - 1}>下一页</button>
      </div>}
    </section>

    <section className={styles.card}>
      <div className={styles.header}>
        <div><h2>诊断包</h2><ShieldCheck size={16} /></div>
        <div className={styles.actions}>
          <button onClick={() => void makePreview()} disabled={busy === 'preview'}>
            {busy === 'preview' ? '生成预览…' : '生成预览'}
          </button>
          <button className="primary" onClick={() => void exportPackage()} disabled={busy === 'export' || !preview}>
            <Save size={16} />{busy === 'export' ? '保存中…' : '保存到本地'}
          </button>
        </div>
      </div>

      <div className={styles.scopes}>
        {CATEGORIES.map(scope => <label key={scope} className={styles.checkbox}>
          <input type="checkbox" checked={selectedScopes.includes(scope)}
            onChange={event => setSelectedScopes(list => event.target.checked ? [...list, scope] : list.filter(item => item !== scope))} />
          {scope}
        </label>)}
      </div>

      {preview && <div className={styles.preview}>
        <p className="text-muted">合计 {preview.totalBytes.toLocaleString()} 字节 · {preview.items.length} 项</p>
        <ul>{preview.items.map(item => <li key={item.name} className={item.included ? styles.included : ''}>
          <span>{item.name}</span>
          <span className={`badge ${item.included ? '' : 'warning'}`}>{item.included ? '包含' : '不包含'}</span>
          <span className="text-muted break-anywhere">{item.note}</span>
        </li>)}</ul>
      </div>}

      {savedPath && <div className={styles.saved} role="status"><Download size={15} />已保存到 <span className="text-mono break-anywhere">{savedPath}</span></div>}
    </section>

    {detail && <Dialog title="事件详情" description={`${detail.categoryKey} · ${levelLabels[detail.level]}`} onClose={() => setDetail(null)}>
      <div className="form-fields">
        <dl className={styles.detail}>
          <dt>时间</dt><dd className="text-mono">{detail.timestamp}</dd>
          <dt>结果</dt><dd>{detail.resultKey}</dd>
          <dt>目标</dt><dd className="break-anywhere">{detail.targetLabel}</dd>
          <dt>耗时</dt><dd>{detail.elapsedMs != null ? `${detail.elapsedMs} ms` : '—'}</dd>
        </dl>
        <h3 className="form-section">安全元数据</h3>
        {Object.keys(detail.safeMetadata ?? {}).length === 0
          ? <p className="text-muted">该事件没有附加元数据。</p>
          : <dl className={styles.detail}>{Object.entries(detail.safeMetadata).map(([key, value]) => <span key={key} className={styles.kv}>
            <dt className="text-mono">{key}</dt><dd className="text-mono break-anywhere">{String(value)}</dd>
          </span>)}</dl>}
        <h3 className="form-section">关联事件 <span>同一目标或同一事务</span></h3>
        {related.length === 0
          ? <p className="text-muted">没有找到同一目标或同一事务的其他事件。</p>
          : <ul className={styles.related}>{related.map((event, index) => <li key={index}>
            <span className="text-mono text-muted">{event.timestamp}</span>
            <span className="break-anywhere">{event.resultKey}</span>
          </li>)}</ul>}
        <div className="actions" style={{ justifyContent: 'flex-end' }}>
          <button onClick={() => setDetail(null)} autoFocus>关闭</button>
        </div>
      </div>
    </Dialog>}

    {confirmClear && <Dialog title="清空日志" busy={busy === 'clear'}
      description={`将清空本工具记录的 ${events.length} 条诊断事件。这不影响 Codex 会话历史，也不影响配置事务与备份记录。`}
      onClose={() => setConfirmClear(false)}>
      <div className="form-fields"><div className="form-footer">
        <span>清空后无法恢复。</span>
        <div className="actions">
          <button onClick={() => setConfirmClear(false)} disabled={busy === 'clear'}>取消</button>
          <button className="danger" autoFocus disabled={busy === 'clear'} onClick={() => void clear()}>
            {busy === 'clear' ? '清空中…' : '清空日志'}
          </button>
        </div>
      </div></div>
    </Dialog>}
  </div>;
}
