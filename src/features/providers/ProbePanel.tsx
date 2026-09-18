import { useState } from 'react';
import { Activity, KeyRound, ListChecks, RefreshCw, Sparkles, TriangleAlert } from 'lucide-react';
import type { DiagnosticEvent, ProbeResult, Provider } from '@/contracts/types';
import { type DesktopClient, type DiscoveredModel, toCoreError } from '@/desktop/client';
import styles from './ProbePanel.module.css';

/** 阶段与状态的可读文案。核心只给 messageKey，中文在文案层拼。 */
const stageLabels: Record<string, string> = {
  connect: '地址可达',
  credential: 'Key 被接受',
  model: '上游存在该模型',
  generate: '真实请求',
};

const stageNotes: Record<string, string> = {
  'probe.connected': '已建立连接',
  'probe.credentialAccepted': 'Key 有效',
  'probe.credentialRejected': 'Key 被拒绝',
  'probe.credentialForbidden': 'Key 没有权限',
  'probe.credentialUnknown': '无法判定（该端点未提供模型列表）',
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

const stateLabels: Record<ProbeResult['stages'][number]['status'], string> = {
  passed: '通过',
  failed: '失败',
  skipped: '跳过',
  running: '进行中',
};

function noteFor(messageKey: string): string {
  return stageNotes[messageKey] ?? messageKey;
}

/**
 * 供应商的连接与发现面板。
 *
 * 两件事分开：`测试连接` 只读（不发真实生成，不产生费用），
 * `发一次真实请求` 才会命中上游并可能计费，所以单独勾选而不是默认打开。
 */
export function ProbePanel({ client, provider, credentialId }: {
  client: DesktopClient; provider: Provider; credentialId: string | null;
}) {
  const [probe, setProbe] = useState<ProbeResult | null>(null);
  const [discovered, setDiscovered] = useState<DiscoveredModel[] | null>(null);
  const [includeGenerate, setIncludeGenerate] = useState(false);
  const [busy, setBusy] = useState('');
  const [error, setError] = useState('');
  const [notice, setNotice] = useState('');
  const [events, setEvents] = useState<DiagnosticEvent[]>([]);

  async function run(label: string, work: () => Promise<void>) {
    setBusy(label); setError(''); setNotice('');
    try { await work(); }
    catch (thrown) { setError(toCoreError(thrown).safeDetails.join('；') || '操作失败。'); }
    finally { setBusy(''); }
  }

  const probeConnection = () => run('probe', async () => {
    if (!credentialId) return;
    const result = await client.startProbe(
      { providerId: provider.id, credentialId },
      { includeGenerate },
    );
    setProbe(result);
    setNotice(result.stages.every(stage => stage.status !== 'failed') ? '探测通过。' : '探测未全部通过，见下方阶段明细。');
    setEvents((await client.listDiagnostics({ level: 'warning' })).items.slice(-5).reverse());
  });

  const discover = () => run('discover', async () => {
    if (!credentialId) return;
    const models = await client.discoverModels(provider.id, credentialId);
    setDiscovered(models);
    setNotice(`上游返回 ${models.length} 个模型；已保存的显示名不会被覆盖。`);
  });

  return <div className={styles.panel}>
    <div className={styles.head}>
      <h3><Activity size={17} />连接检查</h3>
      <div className={styles.actions}>
        <label className={styles.checkbox}>
          <input type="checkbox" checked={includeGenerate} onChange={event => setIncludeGenerate(event.target.checked)} />
          发一次真实请求（可能计费）
        </label>
        <button className="primary" onClick={() => void probeConnection()} disabled={!credentialId || busy === 'probe'}>
          {busy === 'probe' ? '检测中…' : '测试连接'}
        </button>
        <button onClick={() => void discover()} disabled={!credentialId || busy === 'discover'}>
          <Sparkles size={16} />{busy === 'discover' ? '获取中…' : '从上游获取模型'}
        </button>
      </div>
    </div>

    {!credentialId && <div className={styles.note}><KeyRound size={16} /><p>先添加并选择一个 Key，再测试连接。</p></div>}
    {error && <div className="error-message" role="alert">{error}</div>}
    {notice && <div className={styles.notice} role="status">{notice}</div>}

    {probe && <div className={styles.result}>
      <div className={styles.stages}>
        {probe.stages.map(stage => <div key={stage.stageKey} className={`${styles.stage} ${styles[stage.status]}`}>
          <span>{stageLabels[stage.stageKey] ?? stage.stageKey}</span>
          <strong>{stateLabels[stage.status]}</strong>
          <small>{noteFor(stage.messageKey)}</small>
          {stage.elapsedMs != null && <small className="text-mono">{stage.elapsedMs} ms</small>}
        </div>)}
      </div>
      {probe.generated && <div className={styles.warn}><TriangleAlert size={15} />本次探测向供应商发起了真实请求。</div>}
    </div>}

    {discovered && <div className={styles.discovered}>
      <h4><ListChecks size={16} />上游模型 <span className="badge">{discovered.length}</span></h4>
      {discovered.length === 0
        ? <p className="text-muted">上游没有返回任何模型。</p>
        : <ul>{discovered.slice(0, 30).map(model => <li key={model.upstreamId}>
          <span className="text-mono break-anywhere">{model.upstreamId}</span>
          <span className="text-muted break-anywhere">{model.displayName}</span>
          <span className={`badge ${model.alreadySaved ? '' : 'warning'}`}>{model.alreadySaved ? '已添加' : '未添加'}</span>
        </li>)}</ul>}
      {discovered.length > 30 && <p className="text-muted">仅显示前 30 个。</p>}
    </div>}

    {events.length > 0 && <div className={styles.discovered}>
      <h4><RefreshCw size={16} />最近的警告</h4>
      <ul>{events.map(event => <li key={`${event.timestamp}-${event.targetLabel}-${event.resultKey}`}>
        <span className="text-mono text-muted">{event.timestamp}</span>
        <span className="break-anywhere">{event.resultKey}</span>
        <span className="text-muted break-anywhere">{event.targetLabel}</span>
      </li>)}</ul>
    </div>}
  </div>;
}

/** 供其它页面复用的阶段文案，避免两处各写一套。 */
export { stageLabels, stateLabels, noteFor };
