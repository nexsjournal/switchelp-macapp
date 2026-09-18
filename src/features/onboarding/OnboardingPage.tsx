import { useCallback, useEffect, useMemo, useState } from 'react';
import { AlertTriangle, ArrowLeft, ArrowRight, Check, CircleHelp, RefreshCw, Server, ShieldCheck, Sparkles } from 'lucide-react';
import type { CodexInstance, Credential, Model, Provider } from '@/contracts/types';
import { type DesktopClient, toCoreError } from '@/desktop/client';
import styles from './OnboardingPage.module.css';

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

const steps = ['检测 Codex', '添加供应商和模型', '测试并应用'] as const;

const stageNotes: Record<string, string> = {
  'probe.connected': '已建立连接',
  'probe.credentialAccepted': 'Key 有效',
  'probe.credentialRejected': 'Key 被拒绝',
  'probe.credentialForbidden': 'Key 没有该模型权限',
  'probe.credentialUnknown': '该端点未提供模型列表，无法判定',
  'probe.modelFound': '已在上游模型列表中',
  'probe.modelMissing': '上游模型列表里没有这个 ID',
  'probe.modelsUnsupported': '此端点未提供模型列表',
  'probe.generatePassed': '真实请求成功',
  'probe.timedOut': '连接超时',
  'probe.unresolvable': '域名无法解析',
  'probe.upstreamUnreachable': '无法连接上游',
  'probe.skippedNoCredential': '未判定（凭据阶段已失败）',
  'probe.skippedUnreachable': '未执行（地址不可达）',
  'probe.skippedNoModel': '未执行（未指定模型）',
};

function noteFor(messageKey: string): string {
  return stageNotes[messageKey] ?? messageKey;
}

export function OnboardingPage({ client, providers, models, credentialsByProvider, onOpenProviderForm, onOpenModelEditor, onViewDiff, onDismiss }: {
  client: DesktopClient;
  providers: Provider[];
  models: Model[];
  credentialsByProvider: Record<string, Credential[]>;
  onOpenProviderForm: () => void;
  onOpenModelEditor: () => void;
  onViewDiff: () => void;
  onDismiss: () => void;
}) {
  const [step, setStep] = useState(0);
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
    } catch (thrown) { setError(toCoreError(thrown).safeDetails.join('；') || '实例检测失败。'); }
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
      setError(toCoreError(thrown).safeDetails.join('；') || '测试失败。');
      setProbeState('idle');
    }
  };

  const verified = probeState === 'done' && stages.length > 0 && stages.every(stage => stage.status !== 'failed');

  return <div className={styles.page}>
    <header className={styles.header}>
      <div>
        <h1 className="text-page-title">接入向导</h1>
        <p className="text-muted">建立正确的目标实例，并完成第一条可用链路。三步之间可以随时返回。</p>
      </div>
      <button className="text-button" onClick={onDismiss}>稍后再说</button>
    </header>

    <ol className={styles.steps} aria-label="接入步骤">
      {steps.map((label, index) => <li key={label} className={index === step ? styles.current : index < step ? styles.done : ''}
        aria-current={index === step ? 'step' : undefined}>
        <span className={styles.index}>{index < step ? <Check size={13} /> : index + 1}</span>
        <span className={styles.label}>{label}</span>
      </li>)}
    </ol>

    {error && <div className="error-message" role="alert">{error}</div>}

    {step === 0 && <section className={styles.card}>
      <div className={styles.cardHeader}><h2>检测 Codex 实例</h2>
        <button className="text-button" onClick={() => void detect()} disabled={detecting}>
          <RefreshCw size={15} />{detecting ? '检测中…' : '重新检测'}
        </button>
      </div>

      {instances.length === 0 && !detecting ? <div className={styles.empty}>
        <Server size={26} />
        <h3>未检测到 Codex</h3>
        <p>本工具不会自动下载或安装 Codex。请先安装受支持的 Codex 版本，或手动指定应用路径与配置目录后重新检测。</p>
        <div className={styles.manual}>
          <label className={styles.field}>应用路径
            <input aria-label="Codex 应用路径" placeholder="例如 /Applications/ChatGPT.app" value={manualPath} onChange={event => setManualPath(event.target.value)} /></label>
          <button onClick={() => void detect(manualPath.trim() || undefined)} disabled={detecting}>用这个路径检测</button>
        </div>
      </div> : <>
        {instances.length > 1 && <p className={styles.hint}><CircleHelp size={15} />
          检测到 {instances.length} 个实例。请自己选择要用哪一个——本工具不会替你挑一个正在运行的。</p>}
        <ul className={styles.instances}>{instances.map(instance => <li key={instance.id}
          className={instance.id === instanceId ? styles.picked : ''}>
          <label className={styles.instanceChoice}>
            <input type="radio" name="instance" checked={instance.id === instanceId} onChange={() => setInstanceId(instance.id)} />
            <div>
              <strong>{instance.appPath ?? '未检测到应用路径'}</strong>
              <span className="text-mono text-muted break-anywhere">{instance.configFile}</span>
              <div className={styles.tags}>
                <span className="badge">{instance.configExists ? '配置存在' : '配置不存在'}</span>
                <span className="badge">{instance.startupMode === 'not_running' ? '当前未运行' : instance.startupMode}</span>
                <span className="badge">{instance.compatibility === 'unverified' ? '兼容性未验证' : instance.compatibility}</span>
                {instance.conflictingManagers.map(manager => <span key={manager} className="badge warning">其他工具：{manager}</span>)}
              </div>
            </div>
          </label>
        </li>)}</ul>

        {conflicts.length > 0 && <div className={styles.conflict}>
          <AlertTriangle size={16} />
          <div>
            <strong>检测到其他配置管理工具：{conflicts.join('、')}</strong>
            <p>可以继续保存供应商与模型，但应用这一步会先停下来让你处理冲突，前面填的内容不会丢。</p>
          </div>
        </div>}
      </>}
    </section>}

    {step === 1 && <section className={styles.card}>
      <div className={styles.cardHeader}><h2>添加供应商和模型</h2></div>
      <p className={styles.hint}><ShieldCheck size={15} />Key 只在提交那一次传给后端，之后保存在系统凭据库，界面只显示备注和掩码。</p>

      <ul className={styles.checklist}>
        <li className={readyProviders.length > 0 ? styles.ok : ''}>
          <span className={styles.mark}>{readyProviders.length > 0 ? <Check size={13} /> : '1'}</span>
          <div><strong>添加供应商与 Key</strong>
            <span>{readyProviders.length > 0 ? `已添加 ${readyProviders.length} 个供应商` : '还没有供应商'}</span></div>
          <button onClick={onOpenProviderForm}>{readyProviders.length > 0 ? '再添加一个' : '添加供应商'}</button>
        </li>
        <li className={withActiveKey.length > 0 ? styles.ok : ''}>
          <span className={styles.mark}>{withActiveKey.length > 0 ? <Check size={13} /> : '2'}</span>
          <div><strong>为供应商选择当前 Key</strong>
            <span>{withActiveKey.length > 0 ? `${withActiveKey.length} 个供应商已选好 Key` : '还需要在供应商页把某个 Key 设为当前'}</span></div>
          <button onClick={onOpenProviderForm} disabled={!readyProviders.length}>去选择</button>
        </li>
        <li className={catalogModels.length > 0 ? styles.ok : ''}>
          <span className={styles.mark}>{catalogModels.length > 0 ? <Check size={13} /> : '3'}</span>
          <div><strong>添加模型并纳入目录</strong>
            <span>{catalogModels.length > 0 ? `${catalogModels.length} 个模型已纳入待应用目录` : '模型需要填上游 ID 与上下文'}</span></div>
          <button onClick={onOpenModelEditor} disabled={!providers.length}>添加模型</button>
        </li>
      </ul>

      {!canAdvanceFromSetup && <p className={styles.hint}>
        下一步需要至少一个「已选 Key 的供应商」和一个「已纳入目录的模型」。也可以稍后再补，先退出向导。
      </p>}
    </section>}

    {step === 2 && <section className={styles.card}>
      <div className={styles.cardHeader}><h2>测试并应用</h2>
        {verified && <span className={styles.verified}><Check size={14} />只读检查通过</span>}
      </div>

      <div className={styles.testRow}>
        <div>
          <strong>{probeTarget ? `${probeTarget.model.displayName} · ${probeTarget.model.upstreamId}` : '还没有可测试的模型'}</strong>
          <span className="text-muted">只读检查：读取上游模型列表，不发真实生成请求，因此不产生费用。</span>
        </div>
        <button className="primary" onClick={() => void runProbe()} disabled={!probeTarget || probeState === 'running'}>
          <Sparkles size={16} />{probeState === 'running' ? '检查中…' : '测试连接'}
        </button>
      </div>

      {probeState === 'done' && stages.length > 0 && <ul className={styles.stages}>{stages.map(stage => <li key={stage.stageKey} className={styles[stage.status] ?? ''}>
        <strong>{stage.stageKey}</strong>
        <span>{stage.status === 'passed' ? '通过' : stage.status === 'failed' ? '失败' : '跳过'}</span>
        <span className="text-muted">{noteFor(stage.messageKey)}</span>
      </li>)}</ul>}

      {probeState === 'skipped' && <p className={styles.hint}>
        已跳过在线测试。这个模型不会被显示成已验证；之后可以在连接诊断里补测。
      </p>}

      <div className={styles.finish}>
        <div>
          <strong>在 Codex 模型选择器中选择</strong>
          <span className="text-muted">
            {catalogModels.length > 0
              ? `本次将纳入 ${catalogModels.length} 个模型：${catalogModels.map(model => model.displayName).join('、')}`
              : '还没有纳入目录的模型'}
          </span>
          {/* 没有真实加载证据时用等待状态，不做庆祝页。 */}
          <span className={verified ? styles.stateOk : styles.stateWait}>
            {verified ? '连接测试通过；宿主是否已加载仍需在 Codex 侧确认' : '尚未确认 Codex 已加载新目录'}
          </span>
        </div>
        <div className={styles.finishActions}>
          <button onClick={() => { setProbeState('skipped'); }} disabled={probeState === 'done'}>跳过测试</button>
          <button className="primary" onClick={onViewDiff} disabled={!catalogModels.length}>
            查看差异并应用<ArrowRight size={16} />
          </button>
        </div>
      </div>
    </section>}

    <footer className={styles.footer}>
      <button onClick={() => setStep(value => Math.max(0, value - 1))} disabled={step === 0}>
        <ArrowLeft size={16} />上一步
      </button>
      <span className="text-muted">第 {step + 1} / {steps.length} 步</span>
      <button className="primary" onClick={() => setStep(value => Math.min(steps.length - 1, value + 1))} disabled={step === steps.length - 1}>
        下一步<ArrowRight size={16} />
      </button>
    </footer>
  </div>;
}
