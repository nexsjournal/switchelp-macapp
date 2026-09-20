import { useCallback, useEffect, useMemo, useState } from 'react';
import { AlertTriangle, CheckCircle2, FileSearch, History, RefreshCw, ShieldAlert, SlidersHorizontal } from 'lucide-react';
import type { ApplyPlan, ApplyStage, CodexInstance, FieldChange, Model } from '@/contracts/types';
import { type AppliedSummary, type ApplyStatus, type DesktopClient, type InspectResult, toCoreError } from '@/desktop/client';

import { Dialog } from '@/components/Dialog';
import styles from './CodexConfigPage.module.css';

import { t } from '@/i18n';
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

/** 事务阶段的可读文案。 */
function stageLabelOf(stage: string): string {
  const key = `stage.${stage.replace(/_([a-z])/g, (_, letter: string) => letter.toUpperCase())}`;
  const label = t(key);
  return label === key ? stage : label;
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

export function CodexConfigPage({ client, models, summary, onApplied }: {
  client: DesktopClient;
  models: Model[];
  /** 当前已生效的配置；用于三项版本状态。 */
  summary: AppliedSummary | null;
  onApplied?: () => void;
}) {
  const [instances, setInstances] = useState<CodexInstance[]>([]);
  const [instanceId, setInstanceId] = useState('');
  const [manualPath, setManualPath] = useState('');
  const [inspect, setInspect] = useState<InspectResult | null>(null);
  const [showPreview, setShowPreview] = useState(false);
  const [draft, setDraft] = useState<{ kind: 'apply' | 'restore'; plan: ApplyPlan } | null>(null);
  /** 可以应用的模型：纳入目录且没被停用。一个都没有时「应用到 Codex」没有意义。 */
  const applicableCount = models.filter(model => model.inCatalog && model.lifecycle !== 'disabled').length;
  const [status, setStatus] = useState<ApplyStatus | null>(null);
  // 初始即视为「正在检测」：挂载后立刻就会检测，先渲染空态会闪一下。
  const [busy, setBusy] = useState('detect');
  const [error, setError] = useState('');
  /** 核心给出的恢复动作；界面只呈现自己确实能执行的那些。 */
  const [recovery, setRecovery] = useState<string[]>([]);
  const [notice, setNotice] = useState('');
  const [restartConfirm, setRestartConfirm] = useState(false);

  const fail = useCallback((thrown: unknown, fallback: string) => {
    const normalized = toCoreError(thrown);
    setError(normalized.safeDetails.join(t('common.listSeparator')) || fallback);
    setRecovery(normalized.recoveryActions.map(action => action.action));
  }, []);

  const detect = useCallback(async (explicitPath?: string) => {
    setBusy('detect'); setError(''); setNotice('');
    try {
      const found = await client.detectInstances(explicitPath);
      setInstances(found);
      setInstanceId(current => found.some(instance => instance.id === current) ? current : found[0]?.id ?? '');
      setInspect(null); setDraft(null); setStatus(null);
    } catch (thrown) { fail(thrown, t('codex.detectFailed')); }
    finally { setBusy(''); }
  }, [client, fail]);

  useEffect(() => { void detect(); }, [detect]);

  const selected = instances.find(instance => instance.id === instanceId);
  const stage = status?.events.at(-1)?.phase ?? null;
  const awaitingReload = stage === 'awaiting_reload';

  async function run(label: string, work: () => Promise<void>) {
    setBusy(label); setError(''); setRecovery([]); setNotice('');
    try { await work(); }
    catch (thrown) { fail(thrown, t('common.failed')); }
    finally { setBusy(''); }
  }

  const makePlan = (kind: 'apply' | 'restore') => run('plan', async () => {
    const plan = kind === 'apply' ? await client.planApply({ instanceId, draftRevision: '' }) : await client.planRestore(instanceId);
    setDraft({ kind, plan }); setStatus(null);
    setNotice(kind === 'apply' ? t('codex.diffPreviewed') : t('codex.restorePreviewed'));
  });

  /**
   * 把重启结果翻译成一句准确的话。
   *
   * 三种情况分别说清楚，因为用户接下来要做的事不一样：重启成功可以去菜单里找模型；
   * 没退出去要他自己退出；退出但没起来要他自己打开。笼统说「已重启」会让他在一个
   * 没更新的菜单里找模型。
   */
  function restartNotice(report: { quitConfirmed: boolean; quitForced: boolean; launchedConfirmed: boolean } | null, applied: boolean): string {
    if (!report) return t('codex.restartHostFailed');
    if (report.quitConfirmed && report.launchedConfirmed) {
      // 用了兜底信号就说明它没来得及自己退出：未保存的对话可能已经丢了，必须说。
      if (report.quitForced) return t('codex.restartHostForced');
      return applied ? t('codex.appliedAndRestarted') : t('codex.restartHostRequested');
    }
    if (!report.quitConfirmed) return t('codex.restartHostStillRunning');
    return t('codex.restartHostFailed');
  }

  const commit = () => run('commit', async () => {
    if (!draft) return;
    const request = { planId: draft.plan.id, planHash: draft.plan.planHash, idempotencyKey: newIdempotencyKey() };
    const result = draft.kind === 'apply' ? await client.executeApply(request) : await client.executeRestore(request);
    const next = await client.applyStatus(result.operationId);
    setStatus(next);
    if (draft.kind === 'restore') { setNotice(t('stage.restored')); setDraft(null); onApplied?.(); return; }
    onApplied?.();
    // 配置写完了，但 Codex 只在启动时读它：直接重启，省掉「再点一次重启」这一步。
    // 重启失败不影响已经提交的配置，所以这里只降级成提示。
    const report = await client.restartHost(instanceId).catch(() => null);
    setNotice(restartNotice(report, true));
  });

  /**
   * 重启宿主。Codex 只在启动时读 `config.toml`：写完配置不重启，模型不会出现在它的菜单里。
   * 结果按进程是否真的退出、是否真的回来报告，而不是按命令有没有发出去。
   */
  const restartHost = () => run('restart', async () => {
    const report = await client.restartHost(instanceId);
    setRestartConfirm(false);
    setNotice(restartNotice(report, false));
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
  const commitLabel = draft?.kind === 'restore' ? t('codex.confirmRestore')
    : draft?.plan.reloadScope === 'host_reload' ? t('action.applyAndReload') : t('action.applyToCodex');

  if (!instances.length && busy !== 'detect') {
    return <section className={styles.card}>
      {/* 检测失败时这里过去没有错误出口：界面永远停在「未检测到 Codex」，原因看不见。 */}
      {error && <div className="error-message" role="alert">{error}</div>}
      <div className={styles.empty}>
        <FileSearch size={28} />
        <h3>{t('empty.noInstanceTitle')}</h3>
        <p>{t('empty.noInstanceBody')}</p>
      </div>
      <div className={styles.row} style={{ justifyContent: 'center' }}>
        <label className={styles.field}>{t('codex.appPath')}
          <input aria-label={t('codex.appPathLabel')} placeholder="/Applications/ChatGPT.app" value={manualPath} onChange={event => setManualPath(event.target.value)} />
        </label>
        <button onClick={() => void detect(manualPath.trim() || undefined)} disabled={busy === 'detect'}>
          <RefreshCw size={16} />{busy === 'detect' ? t('codex.detecting') : t('codex.detect')}
        </button>
      </div>
    </section>;
  }

  return <div className={styles.page}>
    {error && <div className="error-message" role="alert">{error}</div>}
    {recovery.includes('recompare') && <div className={styles.card}>
      <div className={styles.awaiting} style={{ marginTop: 0 }}>
        <div><strong>{t('stage.conflict')}</strong><span>{t('copy.conflict')}</span></div>
        <div className={styles.actions}>
          <button className="primary" onClick={() => void makePlan('apply')} disabled={busy === 'plan'}>{t('action.recompare')}</button>
        </div>
      </div>
    </div>}
    {notice && <div role="status" className="error-message" style={{ color: 'var(--text-primary)', background: 'var(--bg-elevated)' }}>{notice}</div>}

    <section className={styles.card}>
      <div className={styles.header}>
        <div><SlidersHorizontal size={18} /><h2>{t('codex.instances')}</h2></div>
        <button className="text-button" onClick={() => void detect()} disabled={busy === 'detect'}><RefreshCw size={15} />{t('codex.recheck')}</button>
      </div>
      {instances.length > 1 && <div className={styles.row} style={{ marginBottom: 20 }}>
        <label className={styles.field}>{t('codex.selectInstance')}<select aria-label={t('codex.selectInstance')} value={instanceId} onChange={event => { setInstanceId(event.target.value); setInspect(null); setDraft(null); setStatus(null); }}>
            {instances.map(instance => <option key={instance.id} value={instance.id}>{instance.configFile}</option>)}
          </select>
        </label>
      </div>}
      {selected && <dl className={styles.details}>
        <dt>{t('codex.configDir')}</dt><dd className="text-mono break-anywhere">{selected.configRoot}</dd>
        <dt>{t('codex.configFile')}</dt><dd className="text-mono break-anywhere">{selected.configFile}</dd>
        <dt>CLI</dt><dd className="text-mono break-anywhere">{selected.cliPath ?? t('codex.notDetected')}</dd>
        <dt>{t('codex.compatibility')}</dt><dd>{t(`compat.${selected.compatibility}`)}{selected.blockedReasonKey ? ` · ${selected.blockedReasonKey}` : ''}</dd>
      </dl>}
      <ul className={styles.versions} aria-label={t('codex.versions')}>
        <li>
          <span>{t('codex.savedVersion')}</span>
          <strong>{t('codex.savedVersionValue', { count: models.filter(model => model.inCatalog).length })}</strong>
          <small>{t('codex.savedVersionNote', { total: models.length })}</small>
        </li>
        <li>
          <span>{t('codex.publishedVersion')}</span>
          <strong>{summary ? summary.catalogRevision : t('codex.publishedNever')}</strong>
          <small>{summary ? t('codex.publishedSummary', { count: summary.aliasCount, stage: stageLabelOf(summary.stage) }) : t('codex.publishedNeverNote')}</small>
        </li>
        <li>
          <span>{t('codex.observedVersion')}</span>
          <strong>{t('codex.observedNever')}</strong>
          <small>{t('codex.observedNote')}</small>
        </li>
      </ul>

      <div className={styles.actions} style={{ marginTop: 20 }}>
        <button onClick={loadInspect} disabled={!instanceId || busy === 'inspect'}>{t('codex.checkConfig')}</button>
        <button className="primary" onClick={() => void makePlan('apply')} disabled={!instanceId || busy === 'plan'}>{t('action.applyToCodex')}</button>
        <button onClick={() => void makePlan('restore')} disabled={!instanceId || busy === 'plan'}><History size={16} />{t('action.restorePrevious')}</button>
        <button onClick={() => setRestartConfirm(true)} disabled={!instanceId || busy === 'restart'}><RefreshCw size={15} />{t('action.restartHost')}</button>
      </div>
      {/*
        没有可应用的模型时提前说明，而不是让用户点一个必然失败的按钮。
        按钮本身不禁用：模型列表可能还没加载完，禁用会造成「明明有模型却点不动」。
      */}
      {applicableCount === 0 && <p className={styles.subtle}>{t('codex.noApplicableModels')}</p>}
    </section>

    {inspect && <section className={styles.card}>
      <div className={styles.header}><div><FileSearch size={18} /><h2>{t('codex.inspectTitle')}</h2></div>
        <button className="text-button" onClick={() => setShowPreview(value => !value)}>{showPreview ? t('shell.hidePreview') : t('shell.showPreview')}</button></div>
      <dl className={styles.details}>
        <dt>{t('codex.managedFields')}</dt><dd className="text-mono break-anywhere">{inspect.managedFields.join(t('common.itemSeparator')) || t('codex.nothingWritten')}</dd>
        <dt>{t('codex.otherTools')}</dt><dd>{inspect.conflicts.length ? inspect.conflicts.join(t('common.itemSeparator')) : t('codex.noConflictTool')}</dd>
      </dl>
      {showPreview && <pre className={styles.preview} aria-label={t('codex.redactedPreview')}>{inspect.redactedPreview}</pre>}
    </section>}

    {draft && <section className={styles.card}>
      <div className={styles.header}>
        <div>{draft.kind === 'apply' ? <SlidersHorizontal size={18} /> : <History size={18} />}
          <h2>{draft.kind === 'apply' ? t('codex.diffTitleApply') : t('codex.diffTitleRestore')}</h2></div>
        <span className="badge">{t('codex.changeCount', { count: draft.plan.changes.length })}</span>
      </div>
      <p className={styles.subtle}>{t('codex.targetFile')}<span className="text-mono break-anywhere">{draft.plan.configPath}</span></p>
      {draft.plan.changes.length === 0
        ? <p className={styles.subtle} style={{ marginTop: 16 }}>{t('codex.noFieldDiff')}</p>
        : diff.map(group => <div key={group.key} className={styles.group}>
          <h3>{t(`group.${group.key}`)}<span className="badge">{group.changes.length}</span></h3>
          <table className={styles.changes}>
            <thead><tr><th>{t('codex.changeField')}</th><th>{t('codex.changeBefore')}</th><th>{t('codex.changeAfter')}</th><th>{t('codex.changeReason')}</th></tr></thead>
            <tbody>{group.changes.map(change => <tr key={change.keyPath}>
              <td className="text-mono">{change.keyPath}</td>
              <td><code className="text-muted break-anywhere">{change.before ?? t('codex.notSet')}</code></td>
              <td><code className="break-anywhere">{change.after ?? t('codex.willBeDeleted')}</code></td>
              <td className="text-muted">{reasonLabel(change.reasonKey)}</td>
            </tr>)}</tbody>
          </table>
        </div>)}
      {draft.plan.warnings.length > 0 && <div className={styles.warnings}>
        <AlertTriangle size={15} />{t('codex.compileWarnings')}<ul>{draft.plan.warnings.map(raw => {
          const { label, detail } = warningParts(raw);
          return <li key={raw}><strong>{label}</strong>{t('common.labelSeparator')}{detail}</li>;
        })}</ul>
      </div>}
      <div className={styles.note}><ShieldAlert size={17} /><p>{t('codex.casNote')} {t('codex.applyRestartsHost')}</p></div>
      <div className={styles.actions} style={{ marginTop: 20 }}>
        <button className="primary" onClick={() => void commit()} disabled={busy === 'commit'}>
          {busy === 'commit' ? t('codex.committing') : commitLabel}
        </button>
        <button
          onClick={() => { setDraft(null); setError(''); }}
          disabled={busy === 'commit'}
          title={busy === 'commit' ? t('codex.commitNotCancellable') : undefined}
        >{t('action.cancel')}</button>
        {busy === 'commit' && <span className="text-muted">{t('codex.commitNotCancellable')}</span>}
      </div>
    </section>}

    {status && <section className={styles.card}>
      <div className={styles.header}><div><CheckCircle2 size={18} /><h2>{t('codex.txState')}</h2></div>
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
          <span>{t('codex.awaitingReloadBody')}</span>
        </div>
        <div className={styles.actions}>
          {/* 重启是让 Codex 真正读到新目录的可靠办法，所以放在这一格里。 */}
          <button onClick={() => setRestartConfirm(true)} disabled={busy === 'restart'}><RefreshCw size={15} />{t('action.restartHost')}</button>
          <button className="primary" onClick={() => void confirmReload(true)} disabled={busy === 'confirm'}>{t('codex.reloaded')}</button>
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

    {/* 放在页面顶层：实例卡里的按钮在没有事务时也要能打开它。 */}
      {restartConfirm && <Dialog title={t('action.restartHost')} description={t('codex.restartHostBody')}
        busy={busy === 'restart'} onClose={() => setRestartConfirm(false)}>
        <div className="form-fields"><div className="form-footer">
          <span>{t('codex.restartHostNote')}</span>
          <div className="actions">
            <button onClick={() => setRestartConfirm(false)} disabled={busy === 'restart'}>{t('action.cancel')}</button>
            <button className="primary" autoFocus disabled={busy === 'restart'} onClick={() => void restartHost()}>
              {busy === 'restart' ? t('codex.restarting') : t('action.restartHost')}
            </button>
          </div>
        </div></div>
      </Dialog>}
  </div>;
}
