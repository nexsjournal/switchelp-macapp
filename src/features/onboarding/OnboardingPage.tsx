import { useCallback, useEffect, useMemo, useState } from 'react';
import { AlertTriangle, ArrowLeft, ArrowRight, Check, CircleHelp, RefreshCw, Server, ShieldCheck, Sparkles } from 'lucide-react';
import type { CodexInstance, Credential, Model, Provider } from '@/contracts/types';
import { type DesktopClient, toCoreError } from '@/desktop/client';
import styles from './OnboardingPage.module.css';

import { t } from '@/i18n';
/**
 * 首次接入向导（设计 P01）。
 *
 * 三步可返回：检测 Codex → 添加供应商和模型 → 测试并应用。
 *
 * 三条不能妥协的规则写进实现：
 * - 检测失败只给安装指引与手动定位，**不自动下载 Codex**；
 * - 检测到多个实例时必须让用户选，不替用户挑一个；
 * - 允许跳过在线测试，但**未测试的模型不会被显示为已验证**，完成页也不做庆祝页。
 */

const stepKeys = ['onboarding.step1', 'onboarding.step2', 'onboarding.step3'] as const;

/**
 * 只有少数 messageKey 与文案键不同名——它们复用「建议」里的标题；
 * 其余同名，兜底直接取 messageKey，缺键时 t() 会把键名显式暴露出来。
 */
const stageNoteKeys: Record<string, string> = {
  'probe.credentialRejected': 'advice.credentialRejected.title',
  'probe.credentialForbidden': 'advice.forbidden.title',
  'probe.timedOut': 'advice.timedOut.title',
};

function noteFor(messageKey: string): string {
  return stageNoteKeys[messageKey] ?? messageKey;
}

export function OnboardingPage({ client, providers, models, credentialsByProvider, step, onStepChange, onOpenProviderForm, onOpenModelEditor, onViewDiff, onDismiss }: {
  client: DesktopClient;
  providers: Provider[];
  models: Model[];
  credentialsByProvider: Record<string, Credential[]>;
  /** 当前步由宿主持有：第 2 步会打开整页模型编辑器，那一刻本组件会卸载，步骤不能只活在组件里。 */
  step: number;
  onStepChange: (step: number) => void;
  onOpenProviderForm: () => void;
  onOpenModelEditor: () => void;
  onViewDiff: () => void;
  onDismiss: () => void;
}) {
  const [instances, setInstances] = useState<CodexInstance[]>([]);
  const [instanceId, setInstanceId] = useState('');
  const [manualPath, setManualPath] = useState('');
  const [detecting, setDetecting] = useState(true);
  const [probeState, setProbeState] = useState<'idle' | 'running' | 'done' | 'skipped'>('idle');
  const [stages, setStages] = useState<{ stageKey: string; status: string; messageKey: string }[]>([]);
  const [error, setError] = useState('');

  const detect = useCallback(async (path?: string) => {
    setDetecting(true); setError('');
    try {
      const found = await client.detectInstances(path);
      setInstances(found);
      // 多个实例时不替用户挑：只在只有一个候选时自动选中。
      setInstanceId(current => found.some(item => item.id === current) ? current : (found.length === 1 ? found[0]!.id : ''));
    } catch (thrown) { setError(toCoreError(thrown).safeDetails.join(t('common.listSeparator')) || t('codex.detectFailed')); }
    finally { setDetecting(false); }
  }, [client]);

  useEffect(() => { void detect(); }, [detect]);

  const catalogModels = models.filter(model => model.inCatalog);
  const readyProviders = providers.filter(provider => (credentialsByProvider[provider.id] ?? []).length > 0);
  const withActiveKey = readyProviders.filter(provider => provider.activeCredentialId);
  const conflicts = instances.flatMap(item => item.conflictingManagers);
  const canAdvanceFromSetup = withActiveKey.length > 0 && catalogModels.length > 0;

  const probeTarget = useMemo(() => {
    const model = catalogModels[0];
    if (!model) return null;
    const provider = providers.find(item => item.id === model.providerId);
    const credentials = credentialsByProvider[model.providerId] ?? [];
    const credential = credentials.find(item => item.id === provider?.activeCredentialId) ?? credentials[0];
    if (!provider || !credential) return null;
    return { model, provider, credential };
  }, [catalogModels, providers, credentialsByProvider]);

  const runProbe = async () => {
    if (!probeTarget) return;
    setProbeState('running'); setError('');
    try {
      const report = await client.startProbe(
        { providerId: probeTarget.provider.id, modelId: probeTarget.model.id, credentialId: probeTarget.credential.id },
        { includeGenerate: false },
      );
      setStages(report.stages.map(stage => ({ stageKey: stage.stageKey, status: stage.status, messageKey: stage.messageKey })));
      setProbeState('done');
    } catch (thrown) {
      setError(toCoreError(thrown).safeDetails.join(t('common.listSeparator')) || t('onboarding.testFailed'));
      setProbeState('idle');
    }
  };

  const verified = probeState === 'done' && stages.length > 0 && stages.every(stage => stage.status !== 'failed');

  return <div className={styles.page}>
    <header className={styles.header}>
      <div>
        <h1 className="text-page-title">{t('onboarding.title')}</h1>
        <p className="text-muted">{t('onboarding.subtitle')}</p>
      </div>
      <button className="text-button" onClick={onDismiss}>{t('onboarding.later')}</button>
    </header>

    <ol className={styles.steps} aria-label={t('onboarding.steps')}>
      {stepKeys.map((key, index) => <li key={key} className={index === step ? styles.current : index < step ? styles.done : ''}
        aria-current={index === step ? 'step' : undefined}>
        <span className={styles.index}>{index < step ? <Check size={12} /> : index + 1}</span>
        <span className={styles.label}>{t(key)}</span>
      </li>)}
    </ol>

    {error && <div className="error-message" role="alert">{error}</div>}

    {step === 0 && <section className={styles.card}>
      <div className={styles.cardHeader}><h2>{t('onboarding.detectTitle')}</h2>
        <button className="text-button" onClick={() => void detect()} disabled={detecting}>
          <RefreshCw size={14} />{detecting ? t('codex.detecting') : t('codex.recheck')}
        </button>
      </div>

      {instances.length === 0 && !detecting ? <div className={styles.empty}>
        <Server size={26} />
        <h3>{t('empty.noInstanceTitle')}</h3>
        <p>{t('onboarding.notFoundBody')}</p>
        <div className={styles.manual}>
          <label className={styles.field}>{t('onboarding.appPath')}<input aria-label={t('codex.appPathLabel')} placeholder={t('onboarding.appPathPlaceholder')} value={manualPath} onChange={event => setManualPath(event.target.value)} /></label>
          <button onClick={() => void detect(manualPath.trim() || undefined)} disabled={detecting}>{t('onboarding.detectWithPath')}</button>
        </div>
      </div> : <>
        {instances.length > 1 && <p className={styles.hint}><CircleHelp size={14} />
          {t('onboarding.multiInstance', { count: instances.length })}</p>}
        <ul className={styles.instances}>{instances.map(instance => <li key={instance.id}
          className={instance.id === instanceId ? styles.picked : ''}>
          <label className={styles.instanceChoice}>
            <input type="radio" name="instance" checked={instance.id === instanceId} onChange={() => setInstanceId(instance.id)} />
            <div>
              <strong>{instance.appPath ?? t('settings.noAppPath')}</strong>
              <span className="text-mono text-muted break-anywhere">{instance.configFile}</span>
              <div className={styles.tags}>
                <span className="badge">{instance.configExists ? t('onboarding.configExists') : t('onboarding.configMissing')}</span>
                <span className="badge">{instance.startupMode === 'not_running' ? t('onboarding.notRunning') : instance.startupMode}</span>
                <span className="badge">{t(`compat.${instance.compatibility}`)}</span>
                {instance.conflictingManagers.map(manager => <span key={manager} className="badge warning">{t('onboarding.otherToolBadge', { name: manager })}</span>)}
              </div>
            </div>
          </label>
        </li>)}</ul>

        {/* role="note"：这是一段附注式警告，给它一个稳定的语义边界（测试也据此限定范围）。 */}
        {conflicts.length > 0 && <div className={styles.conflict} role="note">
          <AlertTriangle size={16} />
          <div>
            <strong>{t('onboarding.conflictTitle', { names: conflicts.join(t('common.itemSeparator')) })}</strong>
            <p>{t('onboarding.conflictBody')}</p>
            {/* 说清「在哪」：提示如果只报一个工具名，用户不知道去哪儿删，也不知道删什么。 */}
            <p className={styles.conflictWhere}>{t('onboarding.conflictWhere')}</p>
            <ul className={styles.conflictFiles}>
              {instances.filter(item => item.conflictingManagers.length > 0)
                .map(item => <li key={item.id}><code className="break-anywhere">{item.configFile}</code></li>)}
            </ul>
          </div>
        </div>}
      </>}
    </section>}

    {step === 1 && <section className={styles.card}>
      <div className={styles.cardHeader}><h2>{t('onboarding.setupTitle')}</h2></div>
      <p className={styles.hint}><ShieldCheck size={14} />{t('onboarding.keyHint')}</p>

      <ul className={styles.checklist}>
        <li className={readyProviders.length > 0 ? styles.ok : ''}>
          <span className={styles.mark}>{readyProviders.length > 0 ? <Check size={12} /> : '1'}</span>
          <div><strong>{t('onboarding.addProviderAndKey')}</strong>
            <span>{readyProviders.length > 0 ? t('onboarding.providersAdded', { count: readyProviders.length }) : t('empty.noProviderTitle')}</span></div>
          <button onClick={onOpenProviderForm}>{readyProviders.length > 0 ? t('onboarding.addAnother') : t('action.addProvider')}</button>
        </li>
        <li className={withActiveKey.length > 0 ? styles.ok : ''}>
          <span className={styles.mark}>{withActiveKey.length > 0 ? <Check size={12} /> : '2'}</span>
          <div><strong>{t('onboarding.pickActiveKey')}</strong>
            <span>{withActiveKey.length > 0 ? t('onboarding.keysChosen', { count: withActiveKey.length }) : t('onboarding.needActiveKey')}</span></div>
          <button onClick={onOpenProviderForm} disabled={!readyProviders.length}>{t('onboarding.goPick')}</button>
        </li>
        <li className={catalogModels.length > 0 ? styles.ok : ''}>
          <span className={styles.mark}>{catalogModels.length > 0 ? <Check size={12} /> : '3'}</span>
          <div><strong>{t('onboarding.addModelAndCatalog')}</strong>
            <span>{catalogModels.length > 0 ? t('onboarding.modelsInCatalog', { count: catalogModels.length }) : t('onboarding.modelNeedsId')}</span></div>
          <button onClick={onOpenModelEditor} disabled={!providers.length}>{t('action.addModel')}</button>
        </li>
      </ul>

      {!canAdvanceFromSetup && <p className={styles.hint}>{t('onboarding.needBoth')}</p>}
    </section>}

    {step === 2 && <section className={styles.card}>
      <div className={styles.cardHeader}><h2>{t('onboarding.step3')}</h2>
        {verified && <span className={styles.verified}><Check size={14} />{t('onboarding.readOnlyPassed')}</span>}
      </div>

      <div className={styles.testRow}>
        <div>
          <strong>{probeTarget ? `${probeTarget.model.displayName} · ${probeTarget.model.upstreamId}` : t('onboarding.nothingToTest')}</strong>
          <span className="text-muted">{t('onboarding.testHint')}</span>
        </div>
        <button className="primary" onClick={() => void runProbe()} disabled={!probeTarget || probeState === 'running'}>
          <Sparkles size={16} />{probeState === 'running' ? t('diag.checking') : t('action.testConnection')}
        </button>
      </div>

      {probeState === 'done' && stages.length > 0 && <ul className={styles.stages}>{stages.map(stage => <li key={stage.stageKey} className={styles[stage.status] ?? ''}>
        <strong>{t(`stage.${stage.stageKey}`)}</strong>
        <span>{t(`probeState.${stage.status}`)}</span>
        <span className="text-muted">{t(noteFor(stage.messageKey))}</span>
      </li>)}</ul>}

      {probeState === 'skipped' && <p className={styles.hint}>{t('onboarding.skipped')}</p>}

      {/* 说清「为什么要重启」：向导前面几步都没提过，到这一步才知道应用会重启宿主，会觉得突兀。 */}
      <p className={styles.hint}><RefreshCw size={14} />{t('onboarding.restartNote')}</p>

      <div className={styles.finish}>
        <div>
          <strong>{t('onboarding.pickInCodex')}</strong>
          <span className="text-muted">
            {catalogModels.length > 0
              ? t('onboarding.willInclude', { count: catalogModels.length, names: catalogModels.map(model => model.displayName).join(t('common.itemSeparator')) })
              : t('onboarding.nothingInCatalog')}
          </span>
          {/* 没有真实加载证据时用等待状态，不做庆祝页。 */}
          <span className={verified ? styles.stateOk : styles.stateWait}>
            {verified ? t('onboarding.verifiedState') : t('onboarding.waitingState')}
          </span>
        </div>
        <div className={styles.finishActions}>
          <button onClick={() => { setProbeState('skipped'); }} disabled={probeState === 'done'}>{t('onboarding.skipTest')}</button>
          <button className="primary" onClick={onViewDiff} disabled={!catalogModels.length}>{t('overview.viewDiffAndApply')}<ArrowRight size={16} />
          </button>
        </div>
      </div>
    </section>}

    <footer className={styles.footer}>
      <button onClick={() => onStepChange(Math.max(0, step - 1))} disabled={step === 0}>
        <ArrowLeft size={16} />{t('onboarding.back')}</button>
      <span className="text-muted">{t('onboarding.stepOf', { current: step + 1, total: stepKeys.length })}</span>
      <button className="primary" onClick={() => onStepChange(Math.min(stepKeys.length - 1, step + 1))} disabled={step === stepKeys.length - 1}>{t('onboarding.next')}<ArrowRight size={16} />
      </button>
    </footer>
  </div>;
}
