import { useState } from 'react';
import { Activity, KeyRound, ListChecks, RefreshCw, Sparkles, TriangleAlert } from 'lucide-react';
import type { DiagnosticEvent, ProbeResult, Provider } from '@/contracts/types';
import { type DesktopClient, type DiscoveredModel, toCoreError } from '@/desktop/client';
import styles from './ProbePanel.module.css';

import { t } from '@/i18n';
/** 阶段与状态的可读文案。核心只给 messageKey，中文在文案层拼。 */
const stageKeys: Record<string, string> = {
  connect: 'stage.connect',
  credential: 'stage.credential',
  model: 'stage.model',
  generate: 'stage.generate',
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

const stateKeys: Record<ProbeResult['stages'][number]['status'], string> = {
  passed: 'probeState.passed',
  failed: 'probeState.failed',
  skipped: 'probeState.skipped',
  running: 'probeState.running',
};

/** 返回文案键，调用方在渲染时取文案。 */
function noteFor(messageKey: string): string {
  return stageNoteKeys[messageKey] ?? messageKey;
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
    catch (thrown) { setError(toCoreError(thrown).safeDetails.join(t('common.listSeparator')) || t('common.failed')); }
    finally { setBusy(''); }
  }

  const probeConnection = () => run('probe', async () => {
    if (!credentialId) return;
    const result = await client.startProbe(
      { providerId: provider.id, credentialId },
      { includeGenerate },
    );
    setProbe(result);
    setNotice(result.stages.every(stage => stage.status !== 'failed') ? t('probe.passedAll') : t('probe.partial'));
    setEvents((await client.listDiagnostics({ level: 'warning' })).items.slice(-5).reverse());
  });

  const discover = () => run('discover', async () => {
    if (!credentialId) return;
    const models = await client.discoverModels(provider.id, credentialId);
    setDiscovered(models);
    setNotice(t('providers.discoveredCount', { count: models.length }));
  });

  return <div className={styles.panel}>
    <div className={styles.head}>
      <h3><Activity size={17} />{t('providers.probeHeading')}</h3>
      <div className={styles.actions}>
        <label className={styles.checkbox}>
          <input type="checkbox" checked={includeGenerate} onChange={event => setIncludeGenerate(event.target.checked)} />
          {t('diag.includeGenerate')}
        </label>
        <button className="primary" onClick={() => void probeConnection()} disabled={!credentialId || busy === 'probe'}>
          {busy === 'probe' ? t('codex.detecting') : t('action.testConnection')}
        </button>
        <button onClick={() => void discover()} disabled={!credentialId || busy === 'discover'}>
          <Sparkles size={16} />{busy === 'discover' ? t('providers.fetching') : t('providers.fetchModels')}
        </button>
      </div>
    </div>

    {!credentialId && <div className={styles.note}><KeyRound size={16} /><p>{t('providers.selectKeyFirst')}</p></div>}
    {error && <div className="error-message" role="alert">{error}</div>}
    {notice && <div className={styles.notice} role="status">{notice}</div>}

    {probe && <div className={styles.result}>
      <div className={styles.stages}>
        {probe.stages.map(stage => <div key={stage.stageKey} className={`${styles.stage} ${styles[stage.status]}`}>
          <span>{t(stageKeys[stage.stageKey] ?? stage.stageKey)}</span>
          <strong>{t(stateKeys[stage.status])}</strong>
          <small>{t(noteFor(stage.messageKey))}</small>
          {stage.elapsedMs != null && <small className="text-mono">{stage.elapsedMs} ms</small>}
        </div>)}
      </div>
      {probe.generated && <div className={styles.warn}><TriangleAlert size={15} />{t('probe.generatedNotice')}</div>}
    </div>}

    {discovered && <div className={styles.discovered}>
      <h4><ListChecks size={16} />{t('providers.upstreamModels')} <span className="badge">{discovered.length}</span></h4>
      {discovered.length === 0
        ? <p className="text-muted">{t('providers.noUpstreamModels')}</p>
        : <ul>{discovered.slice(0, 30).map(model => <li key={model.upstreamId}>
          <span className="text-mono break-anywhere">{model.upstreamId}</span>
          <span className="text-muted break-anywhere">{model.displayName}</span>
          <span className={`badge ${model.alreadySaved ? '' : 'warning'}`}>{model.alreadySaved ? t('providers.added') : t('providers.notAdded')}</span>
        </li>)}</ul>}
      {discovered.length > 30 && <p className="text-muted">{t('providers.onlyFirst30')}</p>}
    </div>}

    {events.length > 0 && <div className={styles.discovered}>
      <h4><RefreshCw size={16} />{t('providers.recentWarnings')}</h4>
      <ul>{events.map(event => <li key={`${event.timestamp}-${event.targetLabel}-${event.resultKey}`}>
        <span className="text-mono text-muted">{event.timestamp}</span>
        <span className="break-anywhere">{event.resultKey}</span>
        <span className="text-muted break-anywhere">{event.targetLabel}</span>
      </li>)}</ul>
    </div>}
  </div>;
}

