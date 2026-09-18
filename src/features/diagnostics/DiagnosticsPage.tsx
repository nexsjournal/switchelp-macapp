import { useCallback, useEffect, useState } from 'react';
import { AlertTriangle, Download, Info, RefreshCw, Save, ShieldCheck, XCircle } from 'lucide-react';
import type { DiagnosticEvent, LogLevel } from '@/contracts/types';
import { type DesktopClient, type DiagnosticsPreview, toCoreError } from '@/desktop/client';
import styles from './DiagnosticsPage.module.css';

/**
 * 诊断页：只展示 allowlist 之后的事件，导出前先给预览。
 *
 * 这里刻意不做“清空全部”之类的破坏性操作：诊断的价值在于事后定位，
 * 保留策略由后端按 7 天 / 20 MiB 自动执行。
 */

const levelIcons = { info: Info, warning: AlertTriangle, error: XCircle } as const;

/** 导出范围：与后端的 categoryKey 前缀一致。 */
const scopes = ['gateway', 'apply', 'probe', 'discovery'] as const;

export function DiagnosticsPage({ client }: { client: DesktopClient }) {
  const [events, setEvents] = useState<DiagnosticEvent[]>([]);
  const [level, setLevel] = useState<LogLevel | 'all'>('all');
  const [selected, setSelected] = useState<string[]>(['gateway', 'apply', 'probe', 'discovery']);
  const [preview, setPreview] = useState<DiagnosticsPreview | null>(null);
  const [savedPath, setSavedPath] = useState('');
  const [busy, setBusy] = useState('');
  const [error, setError] = useState('');

  const load = useCallback(async () => {
    setBusy('load'); setError('');
    try {
      const result = await client.listDiagnostics(level === 'all' ? {} : { level });
      setEvents(result.items);
    } catch (thrown) { setError(toCoreError(thrown).safeDetails.join('；') || '诊断事件加载失败。'); }
    finally { setBusy(''); }
  }, [client, level]);

  useEffect(() => { void load(); }, [load]);

  async function run(label: string, work: () => Promise<void>) {
    setBusy(label); setError('');
    try { await work(); }
    catch (thrown) { setError(toCoreError(thrown).safeDetails.join('；') || '操作失败。'); }
    finally { setBusy(''); }
  }

  const makePreview = () => run('preview', async () => {
    setSavedPath('');
    setPreview(await client.previewDiagnostics({ scopes: selected, redactionPreviewHash: '' }));
  });

  const exportPackage = () => run('export', async () => {
    const result = await client.exportDiagnostics({ scopes: selected, redactionPreviewHash: '' });
    setSavedPath(result.savedPath);
  });

  return <div className={styles.page}>
    {error && <div className="error-message" role="alert">{error}</div>}

    <section className={styles.card}>
      <div className={styles.header}>
        <div><h2>诊断事件</h2><span className="badge">{events.length}</span></div>
        <div className={styles.actions}>
          <label className={styles.field}>级别
            <select aria-label="日志级别" value={level} onChange={event => setLevel(event.target.value as LogLevel | 'all')}>
              <option value="all">全部</option>
              <option value="info">信息及以上</option>
              <option value="warning">警告及以上</option>
              <option value="error">仅错误</option>
            </select>
          </label>
          <button className="icon-button" aria-label="刷新诊断事件" onClick={() => void load()} disabled={busy === 'load'}>
            <RefreshCw size={17} className={busy === 'load' ? styles.spin : ''} />
          </button>
        </div>
      </div>

      <p className={styles.policy}>
        只记录时间、模型与供应商标识、阶段、HTTP 状态与耗时、错误码。prompt、completion、工具参数、
        文件内容、Authorization 与完整 URL query 从不写入。
      </p>

      {events.length === 0
        ? <p className={styles.empty}>还没有诊断事件。执行一次应用或测试连接后，这里会出现脱敏后的记录。</p>
        : <ul className={styles.events}>
          {[...events].reverse().slice(0, 200).map((event, index) => {
            const Icon = levelIcons[event.level];
            return <li key={`${event.timestamp}-${event.resultKey}-${index}`} className={styles[event.level]}>
              <Icon size={15} aria-hidden="true" />
              <span className="text-mono text-muted">{event.timestamp}</span>
              <span className={styles.category}>{event.categoryKey}</span>
              <span className="break-anywhere">{event.resultKey}</span>
              <span className="text-muted break-anywhere">{event.targetLabel}</span>
              {event.elapsedMs != null && <span className="text-mono text-muted">{event.elapsedMs} ms</span>}
              <span className={styles.meta}>
                {Object.entries(event.safeMetadata).map(([key, value]) => `${key}=${value}`).join(' · ')}
              </span>
            </li>;
          })}
        </ul>}
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
        {scopes.map(scope => <label key={scope} className={styles.checkbox}>
          <input type="checkbox" checked={selected.includes(scope)}
            onChange={event => setSelected(current => event.target.checked
              ? [...current, scope]
              : current.filter(item => item !== scope))} />
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

      {savedPath && <div className={styles.saved} role="status">
        <Download size={15} />已保存到 <span className="text-mono break-anywhere">{savedPath}</span>
      </div>}
    </section>
  </div>;
}
