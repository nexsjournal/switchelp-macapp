import { useCallback, useEffect, useMemo, useState } from 'react';
import { AlertTriangle, CheckCircle2, FileSearch, History, RefreshCw, ShieldAlert, SlidersHorizontal } from 'lucide-react';
import type { ApplyPlan, ApplyStage, CodexInstance, FieldChange } from '@/contracts/types';
import { type ApplyStatus, type DesktopClient, type InspectResult, toCoreError } from '@/desktop/client';
import { t } from '@/locales/zh-CN';
import styles from './CodexConfigPage.module.css';

/**
 * “Codex 配置”页：检测实例 → 查看差异 → 应用 → 等待 Codex 重新加载 → 还原。
 *
 * 三条硬约束（来自 docs/architecture/02-configuration-lifecycle.md）：
 * - 提交成功只能显示“等待 Codex 重新加载”，只有用户确认后才显示已核验。
 * - 差异、警告、冲突全部来自核心的 ApplyPlan，界面不自行推断。
 * - 任何重新比较都重新生成计划，不重用过期或冲突的计划。
 */

/** 差异展示分组：只对核心给出的 reasonKey 归类，业务判定仍在核心。 */
const groupOrder = ['route', 'catalog', 'policy', 'restore', 'other'] as const;
type GroupKey = (typeof groupOrder)[number];

/** 事务进度条展示顺序，与核心状态机的正常路径一致。 */
const timeline: ApplyStage[] = ['prepared', 'committing', 'awaiting_reload', 'verified'];

function groupOf(reasonKey: string): GroupKey {
  switch (reasonKey) {
    case 'reason.defaultModel':
    case 'reason.providerRoute':
      return 'route';
    case 'reason.catalog':
    case 'reason.gatewayProvider':
      return 'catalog';
    case 'reason.contextOverride':
    case 'reason.reasoningDefault':
      return 'policy';
    case 'reason.restore':
      return 'restore';
    default:
      return 'other';
  }
}

/** 缺失的 reasonKey 回落到通用文案，而不是把内部 key 显示给用户。 */
function reasonLabel(reasonKey: string): string {
  const short = reasonKey.startsWith('reason.') ? reasonKey.slice('reason.'.length) : reasonKey;
  const label = t(`reason.${short}`);
  return label === `reason.${short}` ? t('reason.other') : label;
}

/** ApplyStage 序列化为 snake_case，文案表使用 camelCase。 */
function stageKey(phase: ApplyStage): string {
  return `stage.${phase.replace(/_([a-z])/g, (_, letter: string) => letter.toUpperCase())}`;
}

/**
 * 核心把编译警告拼成 `warning.xxx：详情`。界面必须显示可读文案，
 * 不能把内部 messageKey 直接摆在用户面前；未知 key 回落到通用标签。
 */
function warningParts(raw: string): { label: string; detail: string } {
  const separator = raw.indexOf('：');
  if (separator < 0) return { label: t('warning.other'), detail: raw };
  const key = raw.slice(0, separator);
  const label = t(key);
  return { label: label === key ? t('warning.other') : label, detail: raw.slice(separator + 1) };
}

function newIdempotencyKey(): string {
  return globalThis.crypto?.randomUUID?.() ?? `${Date.now()}-${Math.random().toString(16).slice(2)}`;
}

function groups(changes: FieldChange[]): { key: GroupKey; changes: FieldChange[] }[] {
  return groupOrder
    .map(key => ({ key, changes: changes.filter(change => groupOf(change.reasonKey) === key) }))
    .filter(group => group.changes.length > 0);
}

export function CodexConfigPage({ client, onApplied }: { client: DesktopClient; onApplied?: () => void }) {
  const [instances, setInstances] = useState<CodexInstance[]>([]);
  const [instanceId, setInstanceId] = useState('');
  const [manualPath, setManualPath] = useState('');
  const [inspect, setInspect] = useState<InspectResult | null>(null);
  const [showPreview, setShowPreview] = useState(false);
  const [draft, setDraft] = useState<{ kind: 'apply' | 'restore'; plan: ApplyPlan } | null>(null);
  const [status, setStatus] = useState<ApplyStatus | null>(null);
  const [busy, setBusy] = useState('');
  const [error, setError] = useState('');
  /** 核心给出的恢复动作；界面只呈现自己确实能执行的那些。 */
  const [recovery, setRecovery] = useState<string[]>([]);
  const [notice, setNotice] = useState('');

  const fail = useCallback((thrown: unknown, fallback: string) => {
    const normalized = toCoreError(thrown);
    setError(normalized.safeDetails.join('；') || fallback);
    setRecovery(normalized.recoveryActions.map(action => action.action));
  }, []);

  const detect = useCallback(async (explicitPath?: string) => {
    setBusy('detect'); setError(''); setNotice('');
    try {
      const found = await client.detectInstances(explicitPath);
      setInstances(found);
      setInstanceId(current => found.some(instance => instance.id === current) ? current : found[0]?.id ?? '');
      setInspect(null); setDraft(null); setStatus(null);
    } catch (thrown) { fail(thrown, '实例检测失败。'); }
    finally { setBusy(''); }
  }, [client, fail]);

  useEffect(() => { void detect(); }, [detect]);

  const selected = instances.find(instance => instance.id === instanceId);
  const stage = status?.events.at(-1)?.phase ?? null;
  const awaitingReload = stage === 'awaiting_reload';

  async function run(label: string, work: () => Promise<void>) {
    setBusy(label); setError(''); setRecovery([]); setNotice('');
    try { await work(); }
    catch (thrown) { fail(thrown, '操作失败。'); }
    finally { setBusy(''); }
  }

  const makePlan = (kind: 'apply' | 'restore') => run('plan', async () => {
    const plan = kind === 'apply' ? await client.planApply({ instanceId, draftRevision: '' }) : await client.planRestore(instanceId);
    setDraft({ kind, plan }); setStatus(null);
    setNotice(kind === 'apply' ? '已生成差异预览。确认后才会写入 Codex 配置。' : '已生成还原预览。');
  });

  const commit = () => run('commit', async () => {
    if (!draft) return;
    const request = { planId: draft.plan.id, planHash: draft.plan.planHash, idempotencyKey: newIdempotencyKey() };
    const result = draft.kind === 'apply' ? await client.executeApply(request) : await client.executeRestore(request);
    const next = await client.applyStatus(result.operationId);
    setStatus(next);
    if (draft.kind === 'restore') { setNotice(t('stage.restored')); setDraft(null); onApplied?.(); }
    else onApplied?.();
  });

  const confirmReload = (loaded: boolean) => run('confirm', async () => {
    if (!status) return;
    setStatus(await client.confirmReload(status.operationId, loaded));
    setNotice(loaded ? t('copy.hostLoaded') : t('copy.hostUnobservable'));
  });

  const loadInspect = () => run('inspect', async () => {
    const result = await client.inspectConfig(instanceId);
    setInspect(result); setShowPreview(false);
  });

  const diff = useMemo(() => (draft ? groups(draft.plan.changes) : []), [draft]);
  // 主按钮文案由核心的 reloadScope 决定：需要宿主重载时不写成“应用”。
  const commitLabel = draft?.kind === 'restore' ? '确认恢复'
    : draft?.plan.reloadScope === 'host_reload' ? t('action.applyAndReload') : t('action.applyToCodex');

  if (!instances.length && busy !== 'detect') {
    return <section className={styles.card}>
      <div className={styles.empty}>
        <FileSearch size={28} />
        <h3>{t('empty.noInstanceTitle')}</h3>
        <p>{t('empty.noInstanceBody')}</p>
      </div>
      <div className={styles.row} style={{ justifyContent: 'center' }}>
        <label className={styles.field}>应用或 CLI 路径
          <input aria-label="Codex 应用路径" placeholder="/Applications/ChatGPT.app" value={manualPath} onChange={event => setManualPath(event.target.value)} />
        </label>
        <button onClick={() => void detect(manualPath.trim() || undefined)} disabled={busy === 'detect'}>
          <RefreshCw size={16} />{busy === 'detect' ? '检测中…' : '检测 Codex'}
        </button>
      </div>
    </section>;
  }

  return <div className={styles.page}>
    {error && <div className="error-message" role="alert">{error}</div>}
    {recovery.includes('recompare') && <div className={styles.card}>
      <div className={styles.awaiting} style={{ marginTop: 0 }}>
        <div><strong>{t('stage.conflict')}</strong><span>{t('copy.conflict')} 差异需要按当前文件重新生成。</span></div>
        <div className={styles.actions}>
          <button className="primary" onClick={() => void makePlan('apply')} disabled={busy === 'plan'}>{t('action.recompare')}</button>
        </div>
      </div>
    </div>}
    {notice && <div role="status" className="error-message" style={{ color: 'var(--text-primary)', background: 'var(--bg-elevated)' }}>{notice}</div>}

    <section className={styles.card}>
      <div className={styles.header}>
        <div><SlidersHorizontal size={18} /><h2>Codex 实例</h2></div>
        <button className="text-button" onClick={() => void detect()} disabled={busy === 'detect'}><RefreshCw size={15} />重新检测</button>
      </div>
      {instances.length > 1 && <div className={styles.row} style={{ marginBottom: 20 }}>
        <label className={styles.field}>选择实例
          <select aria-label="选择实例" value={instanceId} onChange={event => { setInstanceId(event.target.value); setInspect(null); setDraft(null); setStatus(null); }}>
            {instances.map(instance => <option key={instance.id} value={instance.id}>{instance.configFile}</option>)}
          </select>
        </label>
      </div>}
      {selected && <dl className={styles.details}>
        <dt>配置目录</dt><dd className="text-mono break-anywhere">{selected.configRoot}</dd>
        <dt>配置文件</dt><dd className="text-mono break-anywhere">{selected.configFile}</dd>
        <dt>CLI</dt><dd className="text-mono break-anywhere">{selected.cliPath ?? '未检测到'}</dd>
        <dt>兼容性</dt><dd>{t(`compat.${selected.compatibility}`)}{selected.blockedReasonKey ? ` · ${selected.blockedReasonKey}` : ''}</dd>
      </dl>}
      <div className={styles.actions} style={{ marginTop: 20 }}>
        <button onClick={loadInspect} disabled={!instanceId || busy === 'inspect'}>检查当前配置</button>
        <button className="primary" onClick={() => void makePlan('apply')} disabled={!instanceId || busy === 'plan'}>{t('action.applyToCodex')}</button>
        <button onClick={() => void makePlan('restore')} disabled={!instanceId || busy === 'plan'}><History size={16} />{t('action.restorePrevious')}</button>
      </div>
    </section>

    {inspect && <section className={styles.card}>
      <div className={styles.header}><div><FileSearch size={18} /><h2>当前配置</h2></div>
        <button className="text-button" onClick={() => setShowPreview(value => !value)}>{showPreview ? '隐藏预览' : '查看脱敏预览'}</button></div>
      <dl className={styles.details}>
        <dt>受管字段</dt><dd className="text-mono break-anywhere">{inspect.managedFields.join('、') || '尚未写入'}</dd>
        <dt>其他工具</dt><dd>{inspect.conflicts.length ? inspect.conflicts.join('、') : '未检测到冲突工具'}</dd>
      </dl>
      {showPreview && <pre className={styles.preview} aria-label="脱敏配置预览">{inspect.redactedPreview}</pre>}
    </section>}

    {draft && <section className={styles.card}>
      <div className={styles.header}>
        <div>{draft.kind === 'apply' ? <SlidersHorizontal size={18} /> : <History size={18} />}
          <h2>{draft.kind === 'apply' ? '应用差异' : '还原差异'}</h2></div>
        <span className="badge">{draft.plan.changes.length} 项变更</span>
      </div>
      <p className={styles.subtle}>目标文件 <span className="text-mono break-anywhere">{draft.plan.configPath}</span></p>
      {draft.plan.changes.length === 0
        ? <p className={styles.subtle} style={{ marginTop: 16 }}>计划中没有字段差异。</p>
        : diff.map(group => <div key={group.key} className={styles.group}>
          <h3>{t(`group.${group.key}`)}<span className="badge">{group.changes.length}</span></h3>
          <table className={styles.changes}>
            <thead><tr><th>字段</th><th>当前值</th><th>应用后</th><th>原因</th></tr></thead>
            <tbody>{group.changes.map(change => <tr key={change.keyPath}>
              <td className="text-mono">{change.keyPath}</td>
              <td><code className="text-muted break-anywhere">{change.before ?? '未设置'}</code></td>
              <td><code className="break-anywhere">{change.after ?? '将被删除'}</code></td>
              <td className="text-muted">{reasonLabel(change.reasonKey)}</td>
            </tr>)}</tbody>
          </table>
        </div>)}
      {draft.plan.warnings.length > 0 && <div className={styles.warnings}>
        <AlertTriangle size={15} /> 编译警告
        <ul>{draft.plan.warnings.map(raw => {
          const { label, detail } = warningParts(raw);
          return <li key={raw}><strong>{label}</strong>：{detail}</li>;
        })}</ul>
      </div>}
      <div className={styles.note}><ShieldAlert size={17} /><p>应用前会比对配置文件摘要；如果其他工具在此期间修改了配置，本次提交会被拒绝并要求重新比较。</p></div>
      <div className={styles.actions} style={{ marginTop: 20 }}>
        <button className="primary" onClick={() => void commit()} disabled={busy === 'commit'}>
          {busy === 'commit' ? '提交中…' : commitLabel}
        </button>
        <button onClick={() => { setDraft(null); setError(''); }}>取消</button>
      </div>
    </section>}

    {status && <section className={styles.card}>
      <div className={styles.header}><div><CheckCircle2 size={18} /><h2>事务状态</h2></div>
        <span className={`badge ${status.open ? 'warning' : ''}`}>{stage ? t(stageKey(stage)) : '—'}</span></div>
      <ol className={styles.stages}>
        {timeline.map(item => {
          const current = timeline.indexOf(stage as ApplyStage);
          const index = timeline.indexOf(item);
          return <li key={item} className={item === stage ? 'current' : index < current ? 'done' : ''}>{t(stageKey(item))}</li>;
        })}
      </ol>
      {awaitingReload && <div className={styles.awaiting}>
        <div>
          <strong>{t('stage.awaitingReload')}</strong>
          <span>{t('copy.hostUnobservable')} 请在 Codex 中重新加载或重开窗口后再确认。</span>
        </div>
        <div className={styles.actions}>
          <button className="primary" onClick={() => void confirmReload(true)} disabled={busy === 'confirm'}>Codex 已重新加载</button>
          <button onClick={() => void confirmReload(false)} disabled={busy === 'confirm'}>{t('action.laterReload')}</button>
        </div>
      </div>}
      {stage === 'conflict' && <div className={styles.awaiting}>
        <div><strong>{t('stage.conflict')}</strong><span>{t('copy.conflict')}</span></div>
        <div className={styles.actions}>
          <button className="primary" onClick={() => void makePlan('apply')} disabled={busy === 'plan'}>{t('action.recompare')}</button>
        </div>
      </div>}
    </section>}
  </div>;
}
