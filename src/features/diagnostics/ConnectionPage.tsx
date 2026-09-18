import { useMemo, useState } from 'react';
import { Activity, Check, ClipboardCopy, Play, TriangleAlert } from 'lucide-react';
import type { Credential, Model, ProbeResult, Provider } from '@/contracts/types';
import { type DesktopClient, toCoreError } from '@/desktop/client';
import styles from './ConnectionPage.module.css';

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

const stageLabels: Record<string, string> = {
  connect: '地址可达',
  credential: 'Key 被接受',
  model: '上游存在该模型',
  generate: '真实请求',
};

const stateLabels: Record<string, string> = {
  passed: '通过',
  failed: '失败',
  skipped: '跳过',
  running: '进行中',
};

const stageNotes: Record<string, string> = {
  'probe.connected': '已建立连接',
  'probe.credentialAccepted': 'Key 有效',
  'probe.credentialRejected': 'Key 被拒绝',
  'probe.credentialForbidden': 'Key 没有该模型权限',
  'probe.credentialUnknown': '该端点未提供模型列表，无法判定',
  'probe.modelFound': '已在上游模型列表中',
  'probe.modelMissing': '上游模型列表里没有这个 ID',
  'probe.modelsUnsupported': '此端点未提供模型列表',
  'probe.modelsUnparsable': '模型列表无法解析',
  'probe.generatePassed': '真实请求成功',
  'probe.timedOut': '连接超时',
  'probe.unresolvable': '域名无法解析',
  'probe.upstreamUnreachable': '无法连接上游',
  'probe.rateLimited': '被上游限流',
  'probe.upstreamRejected': '上游拒绝了请求',
  'probe.upstreamFailed': '上游返回错误',
  'probe.skippedNoCredential': '未判定（凭据阶段已失败）',
  'probe.skippedUnreachable': '未执行（地址不可达）',
  'probe.skippedNoModel': '未执行（未指定模型）',
  'probe.cancelled': '已取消',
};

/** 建议表：每个失败原因给出**可执行**的下一步，而不是重复错误码。 */
const advice: Record<string, { title: string; steps: string[] }> = {
  'probe.unresolvable': { title: '域名解析不了', steps: ['核对该供应商的 API 地址拼写', '确认本机能解析该域名（可能需要代理）', '若是内网地址，确认当前网络可达'] },
  'probe.timedOut': { title: '连接超时', steps: ['确认地址与端口可访问', '如果走代理，检查代理是否在运行', '换一个网络后再试一次'] },
  'probe.upstreamUnreachable': { title: '连不上上游', steps: ['确认地址使用 https（不是 http）', '检查本机防火墙或公司网络是否拦截', '确认该供应商当前没有故障'] },
  'probe.credentialRejected': { title: 'Key 被拒绝', steps: ['在供应商页重新填写这个 Key', '确认 Key 没有被停用或过期', '确认没有把别的供应商的 Key 填到这里'] },
  'probe.credentialForbidden': { title: 'Key 没有该模型权限', steps: ['确认该账号已开通这个模型', '换一个已开通该模型的 Key', '确认模型 ID 与供应商文档完全一致'] },
  'probe.modelMissing': { title: '上游没有这个模型', steps: ['核对模型 ID 的大小写与斜杠（上游 ID 是精确匹配）', '点「从上游获取模型」看上游实际提供哪些 ID', '确认该账号能否访问这个模型'] },
  'probe.modelsUnsupported': { title: '该端点不提供模型列表', steps: ['这属于正常情况，很多兼容端点没有 /models', '手动填写模型 ID，然后用「发一次真实请求」确认可用'] },
  'probe.modelsUnparsable': { title: '模型列表无法解析', steps: ['该端点的 /models 返回的不是 OpenAI 兼容格式', '手动填写模型 ID，跳过自动发现'] },
  'probe.rateLimited': { title: '被上游限流', steps: ['稍等片刻再试', '确认该 Key 的配额与并发限制'] },
  'probe.upstreamRejected': { title: '上游拒绝了请求', steps: ['确认模型 ID 与请求参数符合该供应商要求', '查看日志页里这次请求的 HTTP 状态与错误码'] },
  'probe.upstreamFailed': { title: '上游返回错误', steps: ['这通常是供应商侧故障，稍后重试', '换一个 Key 排除单个 Key 的问题', '若持续失败，到日志页导出诊断包'] },
};

function noteFor(messageKey: string): string {
  return stageNotes[messageKey] ?? messageKey;
}

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

  const provider = providers.find(item => item.id === providerId);

  /** 选择供应商后加载它的模型与 Key；Key 默认取当前使用的那个。 */
  async function pickProvider(id: string) {
    setProviderId(id); setReport(null); setError('');
    setModelId(''); setCredentialId('');
    setModels([]); setCredentials([]);
    try {
      const [allModels, allCredentials] = await Promise.all([client.listModels(), client.listCredentials(id)]);
      const own = allModels.filter(model => model.providerId === id);
      setModels(own);
      setCredentials(allCredentials);
      const active = providers.find(item => item.id === id)?.activeCredentialId;
      setCredentialId(allCredentials.find(item => item.id === active)?.id ?? allCredentials[0]?.id ?? '');
      setModelId(own[0]?.id ?? '');
    } catch (thrown) { setError(toCoreError(thrown).safeDetails.join('；') || '读取供应商数据失败。'); }
  }

  const start = async () => {
    if (!providerId || !credentialId) return;
    setBusy(true); setError(''); setCopied(false);
    try {
      const result = await client.startProbe(
        { providerId, modelId: modelId || undefined, credentialId },
        { includeGenerate },
      );
      setReport(result);
    } catch (thrown) { setError(toCoreError(thrown).safeDetails.join('；') || '探测失败。'); }
    finally { setBusy(false); }
  };

  /** 失败阶段的建议，去重后按阶段顺序展示。 */
  const suggestions = useMemo(() => {
    if (!report) return [];
    const seen = new Set<string>();
    return report.stages
      .filter(stage => stage.status === 'failed')
      .map(stage => ({ stage: stage.stageKey, messageKey: stage.messageKey, ...(advice[stage.messageKey] ?? { title: '未分类失败', steps: ['到日志页查看这次探测的详细事件'] }) }))
      .filter(item => { const key = item.messageKey; if (seen.has(key)) return false; seen.add(key); return true; });
  }, [report]);

  /** 诊断摘要：可以贴到 issue 或发给供应商的纯文本。不含任何密钥。 */
  const summary = useMemo(() => {
    if (!report || !provider) return '';
    const lines = [
      `GPTSwitch 连接诊断`,
      `时间：${report.startedAt}`,
      `供应商：${provider.name}（${provider.endpoint}）`,
      `协议：${provider.protocol === 'responses' ? 'Responses' : 'Chat Completions'}`,
      `模型：${models.find(model => model.id === modelId)?.upstreamId ?? '（未指定）'}`,
      `真实请求：${report.generated ? '已发送（可能计费）' : '未发送'}`,
      '',
      ...report.stages.map(stage => `[${stateLabels[stage.status] ?? stage.status}] ${stageLabels[stage.stageKey] ?? stage.stageKey} — ${noteFor(stage.messageKey)}${stage.elapsedMs != null ? ` (${stage.elapsedMs} ms)` : ''}`),
    ];
    if (suggestions.length) {
      lines.push('', '建议处理：');
      for (const item of suggestions) lines.push(`- ${item.title}：${item.steps.join('；')}`);
    }
    return lines.join('\n');
  }, [report, provider, models, modelId, suggestions]);

  const copySummary = async () => {
    try {
      await navigator.clipboard?.writeText(summary);
      setCopied(true);
    } catch { setError('剪贴板不可用，请手动选择摘要文本。'); }
  };

  return <div className={styles.page}>
    <section className={styles.card}>
      <div className={styles.header}><div><Activity size={18} /><h2>选择检查目标</h2></div></div>
      <div className={styles.pickers}>
        <label>供应商<select aria-label="选择供应商" value={providerId} onChange={event => void pickProvider(event.target.value)}>
          {providers.length === 0 && <option value="">（还没有供应商）</option>}
          {providers.map(item => <option key={item.id} value={item.id}>{item.name}</option>)}
        </select></label>
        <label>模型<select aria-label="选择模型" value={modelId} onChange={event => setModelId(event.target.value)} disabled={!models.length}>
          <option value="">不指定模型</option>
          {models.map(model => <option key={model.id} value={model.id}>{model.displayName} · {model.upstreamId}</option>)}
        </select></label>
        <label>Key<select aria-label="选择 Key" value={credentialId} onChange={event => setCredentialId(event.target.value)} disabled={!credentials.length}>
          {credentials.length === 0 && <option value="">（没有可用的 Key）</option>}
          {credentials.map(item => <option key={item.id} value={item.id}>{item.label} {item.maskedSuffix}</option>)}
        </select></label>
        <label>协议<input readOnly aria-label="接口协议" value={provider ? (provider.protocol === 'responses' ? 'Responses' : 'Chat Completions') : '—'} /></label>
      </div>

      <div className={styles.run}>
        <label className={styles.checkbox}>
          <input type="checkbox" checked={includeGenerate} onChange={event => setIncludeGenerate(event.target.checked)} />
          发一次真实请求（16 Token，会命中上游并可能计费）
        </label>
        <div className={styles.runActions}>
          <span className="text-muted">默认是只读检查：只读取上游模型列表，不发起生成。</span>
          <button className="primary" onClick={() => void start()} disabled={busy || !providerId || !credentialId}>
            <Play size={16} />{busy ? '检查中…' : '开始检查'}
          </button>
        </div>
      </div>

      {error && <div className="error-message" role="alert">{error}</div>}
      {!providerId && <p className="text-muted">先在供应商页添加一个供应商和 Key，然后回到这里检查。</p>}
    </section>

    {report && <div className={styles.result}>
      <section className={styles.card}>
        <div className={styles.header}><div><h2>阶段时间线</h2><span className="badge">{report.targetLabel}</span></div>
          {report.generated && <span className={styles.warn}><TriangleAlert size={14} />本次已发起真实请求</span>}
        </div>
        <ol className={styles.timeline}>
          {report.stages.map(stage => <li key={stage.stageKey} className={styles[stage.status] ?? ''}>
            <span className={styles.marker} aria-hidden="true" />
            <div>
              <strong>{stageLabels[stage.stageKey] ?? stage.stageKey}</strong>
              <span className={styles.state}>{stateLabels[stage.status] ?? stage.status}</span>
              <p>{noteFor(stage.messageKey)}</p>
            </div>
            {stage.elapsedMs != null && <span className="text-mono text-muted">{stage.elapsedMs} ms</span>}
          </li>)}
        </ol>
      </section>

      <section className={styles.card}>
        <div className={styles.header}><div><h2>建议处理</h2></div></div>
        {suggestions.length === 0
          ? <p className={styles.ok}><Check size={15} />所有阶段通过，没有需要处理的问题。</p>
          : <ul className={styles.advice}>{suggestions.map(item => <li key={item.messageKey}>
            <strong>{item.title}</strong>
            <ul>{item.steps.map(step => <li key={step}>{step}</li>)}</ul>
          </li>)}</ul>}
        <div className={styles.summaryActions}>
          <button onClick={() => void copySummary()} disabled={!summary}>
            <ClipboardCopy size={16} />{copied ? '已复制' : '复制诊断摘要'}
          </button>
        </div>
        <textarea className={styles.summary} readOnly aria-label="诊断摘要" value={summary} rows={6} />
      </section>
    </div>}
  </div>;
}
