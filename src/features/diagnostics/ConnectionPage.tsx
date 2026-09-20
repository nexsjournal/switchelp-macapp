import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { Activity, Check, ClipboardCopy, Play, TriangleAlert } from 'lucide-react';
import type { Credential, Model, ProbeResult, Provider } from '@/contracts/types';
import { type DesktopClient, toCoreError } from '@/desktop/client';
import styles from './ConnectionPage.module.css';

import { t, useLocale } from '@/i18n';
/**
 * 连接诊断页（设计 P07）。
 *
 * 与供应商页里那个内联面板的区别：这里是**可选择的完整检查**——先选供应商/模型/Key，
 * 再决定要不要发真实请求，最后给出**结构化建议**而不是一句“失败”。
 *
 * 「建议处理」按探测返回的 messageKey 给出可执行步骤：设计明确要求
 * “无网络、TLS 失败、401、模型不存在、只支持 Chat、工具未返回、SSE 中断、Codex 未加载
 * 分别给可执行建议，不统一显示 Key 无效”。
 */

const stageKeys: Record<string, string> = {
  connect: 'stage.connect',
  credential: 'stage.credential',
  model: 'stage.model',
  generate: 'stage.generate',
};

const stateKeys: Record<string, string> = {
  passed: 'probeState.passed',
  failed: 'probeState.failed',
  skipped: 'probeState.skipped',
  running: 'probeState.running',
};

/**
 * 只有少数 messageKey 与文案键不同名——它们复用「建议」里的标题；
 * 其余同名，`noteFor` 兜底直接取 messageKey，缺键时 t() 会把键名显式暴露出来。
 */
const stageNoteKeys: Record<string, string> = {
  'probe.credentialRejected': 'advice.credentialRejected.title',
  'probe.credentialForbidden': 'advice.forbidden.title',
  'probe.modelsUnparsable': 'advice.modelsUnparsable.title',
  'probe.timedOut': 'advice.timedOut.title',
  'probe.rateLimited': 'advice.rateLimited.title',
  'probe.upstreamRejected': 'advice.upstreamRejected.title',
  'probe.upstreamFailed': 'advice.upstreamFailed.title',
};

function noteFor(messageKey: string): string {
  return stageNoteKeys[messageKey] ?? messageKey;
}

/**
 * 建议表：每个失败原因给出**可执行**的下一步，而不是重复错误码。
 *
 * 存的是文案键而不是文案：模块只加载一次，这里取文案会被冻结在启动时的语言上。
 */
const advice: Record<string, { title: string; steps: string[] }> = {
  'probe.unresolvable': { title: 'advice.unresolvable.title', steps: ['advice.unresolvable.1', 'advice.unresolvable.2', 'advice.unresolvable.3'] },
  'probe.timedOut': { title: 'advice.timedOut.title', steps: ['advice.timedOut.1', 'advice.timedOut.2', 'advice.timedOut.3'] },
  'probe.upstreamUnreachable': { title: 'advice.unreachable.title', steps: ['advice.unreachable.1', 'advice.unreachable.2', 'advice.unreachable.3'] },
  'probe.credentialRejected': { title: 'advice.credentialRejected.title', steps: ['advice.credentialRejected.1', 'advice.credentialRejected.2', 'advice.credentialRejected.3'] },
  'probe.credentialForbidden': { title: 'advice.forbidden.title', steps: ['advice.forbidden.1', 'advice.forbidden.2', 'advice.forbidden.3'] },
  'probe.modelMissing': { title: 'advice.modelMissing.title', steps: ['advice.modelMissing.1', 'advice.modelMissing.2', 'advice.modelMissing.3'] },
  'probe.modelsUnsupported': { title: 'advice.modelsUnsupported.title', steps: ['advice.modelsUnsupported.1', 'advice.modelsUnsupported.2'] },
  'probe.modelsUnparsable': { title: 'advice.modelsUnparsable.title', steps: ['advice.modelsUnparsable.1', 'advice.modelsUnparsable.2'] },
  'probe.rateLimited': { title: 'advice.rateLimited.title', steps: ['advice.rateLimited.1', 'advice.rateLimited.2'] },
  'probe.upstreamRejected': { title: 'advice.upstreamRejected.title', steps: ['advice.upstreamRejected.1', 'advice.upstreamRejected.2'] },
  'probe.upstreamFailed': { title: 'advice.upstreamFailed.title', steps: ['advice.upstreamFailed.1', 'advice.upstreamFailed.2', 'advice.upstreamFailed.3'] },
};

export function ConnectionPage({ client, providers }: { client: DesktopClient; providers: Provider[] }) {
  const [providerId, setProviderId] = useState(providers[0]?.id ?? '');
  const [modelId, setModelId] = useState('');
  const [credentialId, setCredentialId] = useState('');
  const [models, setModels] = useState<Model[]>([]);
  const [credentials, setCredentials] = useState<Credential[]>([]);
  const [includeGenerate, setIncludeGenerate] = useState(false);
  const [report, setReport] = useState<ProbeResult | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const [copied, setCopied] = useState(false);

  const locale = useLocale();
  const provider = providers.find(item => item.id === providerId);

  /** 加载序号：切换供应商或卸载后，旧响应直接丢弃，不覆盖新选择。 */
  const pendingLoad = useRef(0);

  /** 加载某个供应商的模型与 Key；Key 默认取当前使用的那个。 */
  const loadProvider = useCallback(async (id: string) => {
    const ticket = ++pendingLoad.current;
    setModelId(''); setCredentialId('');
    setModels([]); setCredentials([]);
    try {
      const [allModels, allCredentials] = await Promise.all([client.listModels(), client.listCredentials(id)]);
      if (ticket !== pendingLoad.current) return;
      const own = allModels.filter(model => model.providerId === id);
      setModels(own);
      setCredentials(allCredentials);
      const active = providers.find(item => item.id === id)?.activeCredentialId;
      setCredentialId(allCredentials.find(item => item.id === active)?.id ?? allCredentials[0]?.id ?? '');
      setModelId(own[0]?.id ?? '');
    } catch (thrown) {
      if (ticket !== pendingLoad.current) return;
      setError(toCoreError(thrown).safeDetails.join(t('common.listSeparator')) || t('diag.loadFailed'));
    }
  }, [client, providers]);

  function pickProvider(id: string) {
    setProviderId(id); setReport(null); setError(''); setCopied(false);
    void loadProvider(id);
  }

  /**
   * 首屏与供应商列表变化时也要加载：过去只在 `<select>` 的 onChange 里加载，
   * 进页面时已经选中了第一个供应商，但模型与 Key 下拉是空的、开始按钮点不了，
   * 页面上也不说明原因。
   */
  const providerIds = providers.map(item => item.id).join(',');
  useEffect(() => {
    const selected = providers.some(item => item.id === providerId) ? providerId : (providers[0]?.id ?? '');
    if (selected !== providerId) setProviderId(selected);
    if (selected) void loadProvider(selected);
    // 列表变化或组件卸载时让在途加载作废。
    return () => { pendingLoad.current += 1; };
    // 只跟列表身份走：providerId 由用户操作驱动，不放进依赖以免重复加载。
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [providerIds]);

  const start = async () => {
    if (!providerId || !credentialId) return;
    setBusy(true); setError(''); setCopied(false);
    try {
      const result = await client.startProbe(
        { providerId, modelId: modelId || undefined, credentialId },
        { includeGenerate },
      );
      setReport(result);
    } catch (thrown) { setError(toCoreError(thrown).safeDetails.join(t('common.listSeparator')) || t('diag.probeFailed')); }
    finally { setBusy(false); }
  };

  /** 失败阶段的建议，去重后按阶段顺序展示。 */
  const suggestions = useMemo(() => {
    if (!report) return [];
    const seen = new Set<string>();
    return report.stages
      .filter(stage => stage.status === 'failed')
      .map(stage => {
        const keys = advice[stage.messageKey] ?? { title: 'diag.uncategorized', steps: ['diag.seeLogs'] };
        return { messageKey: stage.messageKey, title: t(keys.title), steps: keys.steps.map(step => t(step)) };
      })
      .filter(item => { const key = item.messageKey; if (seen.has(key)) return false; seen.add(key); return true; });
    // `locale` 是依赖：这里缓存的是取好的文案，语言变了必须重算。
  }, [report, locale]);

  /** 诊断摘要：可以贴到 issue 或发给供应商的纯文本。不含任何密钥。 */
  const summary = useMemo(() => {
    if (!report || !provider) return '';
    const lines = [
      t('diag.summaryTitle'),
      t('diag.summaryTime', { time: report.startedAt }),
      t('diag.summaryProvider', { name: provider.name, endpoint: provider.endpoint }),
      t('diag.summaryProtocol', { protocol: provider.protocol === 'responses' ? 'Responses' : 'Chat Completions' }),
      t('diag.summaryModel', { model: models.find(model => model.id === modelId)?.upstreamId ?? t('diag.unspecified') }),
      t('diag.summaryGenerated', { state: report.generated ? t('diag.summaryGeneratedYes') : t('diag.summaryGeneratedNo') }),
      '',
      ...report.stages.map(stage => `[${t(stateKeys[stage.status] ?? stage.status)}] ${t(stageKeys[stage.stageKey] ?? stage.stageKey)} — ${t(noteFor(stage.messageKey))}${stage.elapsedMs != null ? ` (${stage.elapsedMs} ms)` : ''}`),
    ];
    if (suggestions.length) {
      lines.push('', t('diag.summaryAdvice'));
      for (const item of suggestions) lines.push(t('diag.summarySuggestion', { title: item.title, steps: item.steps.join(t('common.listSeparator')) }));
    }
    return lines.join('\n');
  }, [report, provider, models, modelId, suggestions, locale]);

  const copySummary = async () => {
    try {
      await navigator.clipboard?.writeText(summary);
      setCopied(true);
    } catch { setError(t('diag.clipboardFailed')); }
  };

  return <div className={styles.page}>
    <section className={styles.card}>
      <div className={styles.header}><div><Activity size={18} /><h2>{t('diag.title')}</h2></div></div>
      <div className={styles.pickers}>
        <label>{t('editor.provider')}<select aria-label={t('diag.selectProvider')} value={providerId} onChange={event => void pickProvider(event.target.value)}>
          {providers.length === 0 && <option value="">{t('diag.noProviderOption')}</option>}
          {providers.map(item => <option key={item.id} value={item.id}>{item.name}</option>)}
        </select></label>
        <label>{t('diag.model')}<select aria-label={t('diag.selectModel')} value={modelId} onChange={event => setModelId(event.target.value)} disabled={!models.length}>
          <option value="">{t('diag.noModelOption')}</option>
          {models.map(model => <option key={model.id} value={model.id}>{model.displayName} · {model.upstreamId}</option>)}
        </select></label>
        <label>{t('diag.key')}<select aria-label={t('diag.selectKey')} value={credentialId} onChange={event => setCredentialId(event.target.value)} disabled={!credentials.length}>
          {credentials.length === 0 && <option value="">{t('diag.noKeyOption')}</option>}
          {credentials.map(item => <option key={item.id} value={item.id}>{item.label} {item.maskedSuffix}</option>)}
        </select></label>
        <label>{t('diag.protocol')}<input readOnly aria-label={t('providers.protocol')} value={provider ? (provider.protocol === 'responses' ? 'Responses' : 'Chat Completions') : '—'} /></label>
      </div>

      <div className={styles.run}>
        <label className="check-label">
          <input type="checkbox" checked={includeGenerate} onChange={event => setIncludeGenerate(event.target.checked)} />{t('diag.includeGenerate')}</label>
        <div className={styles.runActions}>
          <span className="text-muted">{t('diag.readOnlyNote')}</span>
          <button className="primary" onClick={() => void start()} disabled={busy || !providerId || !credentialId}>
            <Play size={16} />{busy ? t('diag.checking') : t('diag.start')}
          </button>
        </div>
      </div>

      {error && <div className="error-message" role="alert">{error}</div>}
      {!providerId && <p className="text-muted">{t('diag.addProviderFirst')}</p>}
    </section>

    {report && <div className={styles.result}>
      <section className={styles.card}>
        <div className={styles.header}><div><h2>{t('diag.timeline')}</h2><span className="badge">{report.targetLabel}</span></div>
          {report.generated && <span className={styles.warn}><TriangleAlert size={14} />{t('diag.generated')}</span>}
        </div>
        <ol className={styles.timeline}>
          {report.stages.map(stage => <li key={stage.stageKey} className={styles[stage.status] ?? ''}>
            <span className={styles.marker} aria-hidden="true" />
            <div>
              <strong>{t(stageKeys[stage.stageKey] ?? stage.stageKey)}</strong>
              <span className={styles.state}>{t(stateKeys[stage.status] ?? stage.status)}</span>
              <p>{t(noteFor(stage.messageKey))}</p>
            </div>
            {stage.elapsedMs != null && <span className="text-mono text-muted">{stage.elapsedMs} ms</span>}
          </li>)}
        </ol>
      </section>

      <section className={styles.card}>
        <div className={styles.header}><div><h2>{t('diag.advice')}</h2></div></div>
        {suggestions.length === 0
          ? <p className={styles.ok}><Check size={15} />{t('diag.allPassed')}</p>
          : <ul className={styles.advice}>{suggestions.map(item => <li key={item.messageKey}>
            <strong>{item.title}</strong>
            <ul>{item.steps.map(step => <li key={step}>{step}</li>)}</ul>
          </li>)}</ul>}
        <div className={styles.summaryActions}>
          <button onClick={() => void copySummary()} disabled={!summary}>
            <ClipboardCopy size={16} />{copied ? t('diag.copied') : t('diag.copySummary')}
          </button>
        </div>
        <textarea className={styles.summary} readOnly aria-label={t('diag.summary')} value={summary} rows={6} />
      </section>
    </div>}
  </div>;
}
