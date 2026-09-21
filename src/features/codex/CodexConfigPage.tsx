import { useCallback, useEffect, useState } from 'react';
import { CheckCircle2, FileSearch, History, Layers, RefreshCw, SlidersHorizontal } from 'lucide-react';
import type { ApplyPlan, ApplyStage, CodexInstance, Model } from '@/contracts/types';
import { isCoreError, type AppliedSummary, type ApplyStatus, type CoexistState, type DesktopClient, type InspectResult, toCoreError } from '@/desktop/client';

import { Dialog } from '@/components/Dialog';
import { FieldHelp } from '@/components/FieldHelp';
import { ApplyConfirmDialog } from './ApplyConfirmDialog';
import { newIdempotencyKey } from './idempotency';
import { showToast } from '@/components/Toast';
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

/** 事务进度条展示顺序，与核心状态机的正常路径一致。 */
const timeline: ApplyStage[] = ['prepared', 'committing', 'awaiting_reload', 'verified'];



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
  const [restartConfirm, setRestartConfirm] = useState(false);
  /** 共存模式的状态。`enabled` 是意图，`hostUnderBridge` 是事实。 */
  const [coexist, setCoexist] = useState<CoexistState | null>(null);

  const fail = useCallback((thrown: unknown, fallback: string) => {
    const normalized = toCoreError(thrown);
    setError(normalized.safeDetails.join(t('common.listSeparator')) || fallback);
    setRecovery(normalized.recoveryActions.map(action => action.action));
  }, []);

  const detect = useCallback(async (explicitPath?: string) => {
    setBusy('detect'); setError('');
    try {
      const found = await client.detectInstances(explicitPath);
      setInstances(found);
      setInstanceId(current => found.some(instance => instance.id === current) ? current : found[0]?.id ?? '');
      setInspect(null); setDraft(null); setStatus(null);
    } catch (thrown) { fail(thrown, t('codex.detectFailed')); }
    finally { setBusy(''); }
  }, [client, fail]);

  useEffect(() => { void detect(); }, [detect]);

  // 共存状态跟着实例走：换了实例要重新问一次，否则会显示上一个实例的答案。
  useEffect(() => {
    if (!instanceId) { setCoexist(null); return; }
    let alive = true;
    client.coexistStatus(instanceId)
      .then(next => { if (alive) setCoexist(next); })
      .catch(() => { if (alive) setCoexist(null); });
    return () => { alive = false; };
  }, [client, instanceId]);

  const selected = instances.find(instance => instance.id === instanceId);
  const stage = status?.events.at(-1)?.phase ?? null;
  const awaitingReload = stage === 'awaiting_reload';

  async function run(label: string, work: () => Promise<void>) {
    setBusy(label); setError(''); setRecovery([]);
    try { await work(); }
    catch (thrown) { fail(thrown, t('common.failed')); }
    finally { setBusy(''); }
  }

  /** 生成计划本身（不弹窗、不提示），重生成时也走它。 */
  const planOf = (kind: 'apply' | 'restore') => kind === 'apply'
    ? client.planApply({ instanceId, draftRevision: '' })
    : client.planRestore(instanceId);

  const makePlan = (kind: 'apply' | 'restore') => run('plan', async () => {
    const plan = await planOf(kind);
    setDraft({ kind, plan }); setStatus(null);
    showToast(kind === 'apply' ? t('codex.diffPreviewed') : t('codex.restorePreviewed'), 'info');
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

  /**
   * 这句话该用什么口气。
   *
   * 「重启成功」是成功，「没能重启 / 退出了没起来」是失败——**「被强制结束」也是成功但要说清代价**，
   * 所以它跟着成功走（信息量在文案里，不靠颜色）。语气错了会让人往错的方向处理。
   */
  function restartTone(report: { quitConfirmed: boolean; launchedConfirmed: boolean } | null): 'success' | 'danger' {
    return report && report.quitConfirmed && report.launchedConfirmed ? 'success' : 'danger';
  }

  /**
   * 还原后的结论：说清「撤销了什么」以及「Codex 回到哪里」。
   *
   * 被外部改过的字段按当前值保留（核心的三方比较），数量必须报出来——否则用户会以为
   * 还原漏了东西。重启的三种结果复用同一套说法：配置撤销了但没重启，Codex 里看不出变化。
   */
  function restoreNotice(kept: number, report: { quitConfirmed: boolean; launchedConfirmed: boolean } | null): string {
    const suffix = kept > 0 ? t('codex.restoreKept', { count: kept }) : '';
    if (!report || !report.quitConfirmed) return t('codex.restoreRestartFailed', { suffix });
    if (!report.launchedConfirmed) return t('codex.restoreRestartFailed', { suffix });
    return t('codex.restoredAndRestarted', { suffix });
  }

  const execute = (kind: 'apply' | 'restore', plan: ApplyPlan) => {
    const request = { planId: plan.id, planHash: plan.planHash, idempotencyKey: newIdempotencyKey() };
    return kind === 'apply' ? client.executeApply(request) : client.executeRestore(request);
  };

  /**
   * 提交计划。
   *
   * 计划带的是「生成时那份配置的哈希」：**Codex 自己也会写 config.toml**（启动时补
   * `[projects.*]`、改自己的设置），所以「先生成计划 → 再重启 Codex → 再提交」必然被判成
   * 配置已变更。让用户去理解这套 CAS 是没道理的——重新生成一次计划再提交即可：
   * 应用与还原都只碰本工具自己的受管字段，重放是安全的。
   */
  const commit = () => run('commit', async () => {
    if (!draft) return;
    const kind = draft.kind;
    const result = await execute(kind, draft.plan).catch(async (thrown) => {
      if (!isCoreError(thrown) || thrown.code !== 'CONFIG_CHANGED') throw thrown;
      const fresh = await planOf(kind);
      setDraft({ kind, plan: fresh });
      showToast(t('codex.replanned'), 'info');
      return execute(kind, fresh);
    });
    const next = await client.applyStatus(result.operationId);
    setStatus(next);
    if (kind === 'restore') {
      // 还原之后也必须重启 Codex：它只在启动时读配置，不重启就还是旧的那一套
      // （用户以为还原没生效，其实配置已经撤销了）。
      setDraft(null);
      onApplied?.();
      const kept = draft.plan.warnings.length;
      const restarted = await client.restartHost(instanceId).catch(() => null);
      showToast(restoreNotice(kept, restarted), restartTone(restarted));
      return;
    }
    // 提交成功就关掉差异弹窗：它现在是模态，留着会挡住事务状态那张卡。
    setDraft(null);
    onApplied?.();
    // 配置写完了，但 Codex 只在启动时读它：直接重启，省掉「再点一次重启」这一步。
    // 重启失败不影响已经提交的配置，所以这里只降级成提示。
    const report = await client.restartHost(instanceId).catch(() => null);
    showToast(restartNotice(report, true), restartTone(report));
  });

  /**
   * 重启宿主。Codex 只在启动时读 `config.toml`：写完配置不重启，模型不会出现在它的菜单里。
   * 结果按进程是否真的退出、是否真的回来报告，而不是按命令有没有发出去。
   */
  const restartHost = () => run('restart', async () => {
    const report = await client.restartHost(instanceId);
    setRestartConfirm(false);
    showToast(restartNotice(report, false), restartTone(report));
  });

  /**
   * 开/关共存模式。
   *
   * 开启之后必须再走一次「发布」：模型要写进托管 profile，宿主也得带着 bridge 起来。
   * 所以开启成功就直接接着生成计划（差异弹窗会让用户看清这次要写什么），
   * 关闭则要立刻重启宿主——不重启的话，正在运行的那个宿主还挂在 bridge 上。
   */
  const toggleCoexist = (enabled: boolean) => run(enabled ? 'coexist-on' : 'coexist-off', async () => {
    const next = await client.setCoexist(instanceId, enabled);
    setCoexist(next);
    if (enabled) {
      showToast(t('codex.coexistEnabledThenApply'), 'info');
      await makePlan('apply');
      return;
    }
    const report = await client.restartHost(instanceId).catch(() => null);
    showToast(`${t('codex.coexistDisabledToast')} ${restartNotice(report, false)}`, restartTone(report));
  });

  /**
   * 用当前的原生配置重建托管 profile 的底子，然后重新发布一次。
   *
   * 「底子」是开启共存时复制的那一份快照：用户在原生配置里加了插件/项目信任之后，
   * 想让走我们模型的那些会话也带上，就得重新复制一次。
   */
  const resyncCoexist = () => run('coexist-resync', async () => {
    const next = await client.resyncCoexist(instanceId);
    setCoexist(next);
    showToast(t('codex.coexistResynced'), 'info');
    await makePlan('apply');
  });

  const confirmReload = (loaded: boolean) => run('confirm', async () => {
    if (!status) return;
    setStatus(await client.confirmReload(status.operationId, loaded));
    showToast(loaded ? t('copy.hostLoaded') : t('copy.hostUnobservable'), loaded ? 'success' : 'info');
  });

  const loadInspect = () => run('inspect', async () => {
    const result = await client.inspectConfig(instanceId);
    setInspect(result); setShowPreview(false);
  });

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

    {/*
      共存模式的开关放在实例卡**之前**：它决定「应用」把模型写到哪里（用户的
      ~/.codex 还是应用数据目录里的托管 profile），而不是一个附属设置。
    */}
    <section className={styles.card}>
      <div className={styles.header}>
        <div>
          <Layers size={18} /><h2>{t('codex.coexistHeading')}</h2>
          {/* 语义色沿用既有约定：达成用户所要的状态用成功色，未开启保持中性。 */}
          {coexist && <span className={`badge${coexist.enabled ? ' success' : ''}`}>
            {coexist.enabled ? t('codex.coexistOn') : t('codex.coexistOff')}
          </span>}
        </div>
        <div className={styles.actions}>
          {coexist?.enabled && <button className="text-button" disabled={busy === 'coexist-resync' || busy === 'plan'}
            onClick={() => void resyncCoexist()}>
            <RefreshCw size={14} />{t('codex.coexistResync')}
          </button>}
          <button
            className={coexist?.enabled ? '' : 'primary'}
            disabled={!instanceId || !coexist || busy.startsWith('coexist-') || (!coexist.enabled && !coexist.ready)}
            onClick={() => void toggleCoexist(!(coexist?.enabled ?? false))}>
            {busy === 'coexist-on' ? t('codex.detecting') : busy === 'coexist-off'
              ? t('codex.restarting')
              : coexist?.enabled ? t('codex.coexistDisable') : t('codex.coexistEnable')}
          </button>
        </div>
      </div>
      <p className={styles.subtle}>{t('codex.coexistIntro')}</p>
      {coexist && <>
        <dl className={styles.details}>
          <dt>{t('codex.coexistBridge')}</dt>
          <dd className="text-mono break-anywhere">
            {coexist.bridgeReady
              ? coexist.bridgePath
              : coexist.bridgeDetail && (() => { const label = t(coexist.bridgeDetail!); return label === coexist.bridgeDetail ? coexist.bridgeDetail : label; })()}
          </dd>
          {/*
           * 「托管 profile 是什么、跟原生配置什么关系」原本是卡片下半段的一整段说明。
           * 它解释的是**这一个字段**，所以跟着这个字段走（ⓘ），而不是让整张卡片为它变长——
           * 用户原话：这张卡「字有点太多了」。
           */}
          <dt><span className="field-label">{t('codex.coexistManagedHome')}<FieldHelp text={t('codex.coexistResyncNote')} /></span></dt>
          <dd className="text-mono break-anywhere">{coexist.managedHome}</dd>
        </dl>
        {/* 事实行：宿主到底有没有跑在 bridge 上。意图与事实分开说，不给含糊话。 */}
        {coexist.enabled && <p className={styles.subtle}>
          {coexist.hostUnderBridge === true ? t('codex.coexistFactRunning')
            : coexist.hostUnderBridge === false ? t('codex.coexistFactStale')
            : t('codex.coexistFactUnknown')}
        </p>}
        {!coexist.ready && coexist.blockedReason && <p className={styles.subtle}>{t('codex.coexistBlocked', {
          // 核心给的是 messageKey；翻不出来就原样显示，绝不把 key 当成理由摆出来。
          reason: t(coexist.blockedReason) === coexist.blockedReason ? coexist.blockedReason : t(coexist.blockedReason),
        })}</p>}
      </>}
    </section>

    <section className={styles.card}>
      <div className={styles.header}>
        <div><SlidersHorizontal size={18} /><h2>{t('codex.instances')}</h2></div>
        <button className="text-button" onClick={() => void detect()} disabled={busy === 'detect'}><RefreshCw size={14} />{t('codex.recheck')}</button>
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
        <button onClick={() => void makePlan('restore')} disabled={!instanceId || busy === 'plan' || (coexist?.enabled ?? false)}
          title={coexist?.enabled ? t('codex.coexistRestoreHint') : undefined}>
          <History size={16} />{t('action.restorePrevious')}
        </button>
        <button onClick={() => setRestartConfirm(true)} disabled={!instanceId || busy === 'restart'}><RefreshCw size={14} />{t('action.restartHost')}</button>
      </div>
      {/*
        没有可应用的模型时提前说明，而不是让用户点一个必然失败的按钮。
        按钮本身不禁用：模型列表可能还没加载完，禁用会造成「明明有模型却点不动」。
      */}
      {applicableCount === 0 && <p className={styles.subtle}>{t('codex.noApplicableModels')}</p>}
      {coexist?.enabled && <p className={styles.subtle}>{t('codex.coexistRestoreHint')}</p>}
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

    {/* 差异与确认复用一个组件：待应用条走的是同一套「看得清才让写」的流程。 */}
    {draft && <ApplyConfirmDialog plan={draft.plan} kind={draft.kind} busy={busy === 'commit'} error={error}
      commitLabel={commitLabel} models={models} coexist={coexist?.enabled ?? false}
      onConfirm={() => void commit()} onClose={() => { setDraft(null); setError(''); }} />}

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
          <button onClick={() => setRestartConfirm(true)} disabled={busy === 'restart'}><RefreshCw size={14} />{t('action.restartHost')}</button>
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
      {restartConfirm && <Dialog width="narrow" title={t('action.restartHost')} description={t('codex.restartHostBody')}
        busy={busy === 'restart'} onClose={() => setRestartConfirm(false)} footer={<footer className="form-footer">
          <span>{t('codex.restartHostNote')}</span>
          <div className="actions">
            <button onClick={() => setRestartConfirm(false)} disabled={busy === 'restart'}>{t('action.cancel')}</button>
            <button className="primary" autoFocus disabled={busy === 'restart'} onClick={() => void restartHost()}>
              {busy === 'restart' ? t('codex.restarting') : t('action.restartHost')}
            </button>
          </div>
        </footer>}
        />}
  </div>;
}
