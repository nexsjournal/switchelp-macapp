import { useMemo } from 'react';
import { Activity, ArrowRight, Boxes, CircleHelp, KeyRound, Plus, Server, Settings2, ShieldCheck } from 'lucide-react';
import type { Credential, Model, Provider } from '@/contracts/types';
import type { AppliedSummary, GatewayReport } from '@/desktop/client';
import styles from './OverviewPage.module.css';

/**
 * 概览页（设计 P02）。
 *
 * 两条原则写进实现：
 * - **不谎称**：当前配置卡显示的是本工具**已发布**的默认路由，不声称这是 Codex 当前每个会话在用的模型；
 *   “模型调用”在没有测试记录时显示未测试，而不是显示通过。
 * - **不展示没有可靠来源的数字**：不出现余额、成功率、Token 节省这类估算。
 */

function relativeTime(value: string | null | undefined): string {
  if (!value) return '尚未测试';
  const stamp = Date.parse(value);
  if (Number.isNaN(stamp)) return '尚未测试';
  const minutes = Math.floor((Date.now() - stamp) / 60_000);
  if (minutes < 1) return '刚刚';
  if (minutes < 60) return `${minutes} 分钟前`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours} 小时前`;
  return `${Math.floor(hours / 24)} 天前`;
}

/** Codex 侧的加载状态：只由事务阶段推断，不猜宿主行为。 */
function codexState(summary: AppliedSummary | null, gateway: GatewayReport | null): { text: string; tone: 'ok' | 'warn' | 'muted' } {
  if (!gateway?.running) return { text: '网关未启动', tone: 'warn' };
  if (!summary) return { text: '尚未应用', tone: 'muted' };
  if (summary.stage === 'Verified') return { text: '已核验加载', tone: 'ok' };
  if (summary.stage === 'Pending') return { text: '约定稍后加载', tone: 'warn' };
  return { text: '等待 Codex 重新加载', tone: 'warn' };
}

export function OverviewPage({ providers, models, credentialsByProvider, gateway, summary, pendingCount, onNavigate, onAddProvider }: {
  providers: Provider[];
  models: Model[];
  credentialsByProvider: Record<string, Credential[]>;
  gateway: GatewayReport | null;
  summary: AppliedSummary | null;
  pendingCount: number;
  onNavigate: (page: 'providers' | 'models' | 'codexConfig' | 'diagnostics' | 'settings') => void;
  onAddProvider: () => void;
}) {
  /** 后续请求会用哪个 Key：取已发布默认模型所属供应商的当前 Key。 */
  const routing = useMemo(() => {
    if (!summary?.defaultModel) return null;
    const model = models.find(item => item.catalogAlias === summary.defaultModel);
    if (!model) return null;
    const provider = providers.find(item => item.id === model.providerId);
    const credentials = credentialsByProvider[model.providerId] ?? [];
    const active = credentials.find(item => item.id === provider?.activeCredentialId);
    return { model, provider, active };
  }, [summary, models, providers, credentialsByProvider]);

  const recentDetection = useMemo(() => {
    const stamps = Object.values(credentialsByProvider)
      .flat()
      .map(credential => credential.lastVerifiedAt)
      .filter((value): value is string => Boolean(value))
      .map(value => Date.parse(value))
      .filter(value => !Number.isNaN(value));
    return stamps.length ? new Date(Math.max(...stamps)).toISOString() : null;
  }, [credentialsByProvider]);

  const loading = codexState(summary, gateway);

  if (!providers.length) {
    return <section className={styles.empty}>
      <Server size={30} />
      <h3>添加第一个供应商</h3>
      <p>填入 API 地址与 Key，再添加要使用的模型，然后应用到 Codex。本工具不会自动下载或安装 Codex。</p>
      <button className="primary" onClick={onAddProvider}><Plus size={17} />添加供应商</button>
    </section>;
  }

  return <div className={styles.page}>
    <div className={styles.grid}>
      <section className={styles.card}>
        <div className={styles.cardHeader}><h2>当前配置</h2>
          {summary && <span className="badge">{summary.catalogRevision}</span>}
        </div>
        {routing ? <>
          <p className={styles.route}>
            <strong>{routing.model.displayName}</strong>
            <span className="text-muted">{routing.provider?.name ?? '未知供应商'} · {routing.model.upstreamId}</span>
          </p>
          <dl className={styles.rows}>
            <dt>后续请求</dt>
            <dd>{routing.active ? `${routing.active.label} ${routing.active.maskedSuffix}` : '未选择 Key'}</dd>
            <dt>目录模型</dt>
            <dd>{summary?.aliasCount ?? 0} 个已发布</dd>
          </dl>
        </> : <p className={styles.muted}>
          还没有已生效的配置。下面有 {pendingCount} 个模型等待应用；应用后这里会显示本工具设置的默认路由。
        </p>}
        <div className={styles.cardActions}>
          <button onClick={() => onNavigate('models')}>查看模型</button>
          <button className="primary" onClick={() => onNavigate('codexConfig')}>Codex 配置</button>
        </div>
        <p className={styles.note}><ShieldCheck size={15} />
          这里显示的是**本工具已发布**的默认路由，不代表 Codex 每个会话正在使用的模型。切换只影响后续新请求。</p>
      </section>

      <section className={styles.card}>
        <div className={styles.cardHeader}><h2>连接状态</h2><Activity size={17} /></div>
        <ul className={styles.status}>
          <li>
            <span className={`${styles.dot} ${gateway?.running ? styles.ok : styles.warn}`} aria-hidden="true" />
            <div><strong>本地服务</strong><span>{gateway?.running ? `运行中 · 127.0.0.1:${gateway.port}` : '未启动'}</span></div>
          </li>
          <li>
            <span className={`${styles.dot} ${styles.muted}`} aria-hidden="true" />
            <div><strong>模型调用</strong><span>未测试<span className="text-muted">在连接诊断里跑一次只读检查</span></span></div>
          </li>
          <li>
            <span className={`${styles.dot} ${loading.tone === 'ok' ? styles.ok : loading.tone === 'warn' ? styles.warn : styles.muted}`} aria-hidden="true" />
            <div><strong>Codex 加载</strong><span>{loading.text}</span></div>
          </li>
        </ul>
        {gateway?.error && <p className={styles.gatewayError} role="alert">{gateway.error}</p>}
        <div className={styles.cardActions}>
          <button onClick={() => onNavigate('diagnostics')}>连接诊断</button>
          <button onClick={() => onNavigate('settings')}><Settings2 size={16} />网关设置</button>
        </div>
      </section>
    </div>

    <section className={styles.card}>
      <div className={styles.cardHeader}>
        <h2>供应商</h2>
        <span className="text-muted">最近检测 {relativeTime(recentDetection)}</span>
      </div>
      <ul className={styles.providers}>{providers.slice(0, 5).map(provider => {
        const credentials = credentialsByProvider[provider.id] ?? [];
        const active = credentials.find(item => item.id === provider.activeCredentialId);
        const count = models.filter(model => model.providerId === provider.id).length;
        const tone = !credentials.length ? styles.muted : active ? styles.ok : styles.warn;
        const label = !credentials.length ? '待填写 Key' : active ? `${active.label} · ${active.status === 'verified' ? '已验证' : '未检测'}` : '未选择 Key';
        return <li key={provider.id}>
          <div className={styles.monogram}>{provider.name.slice(0, 1)}</div>
          <div className={styles.providerName}><strong>{provider.name}</strong>
            <span>{credentials.length} 个 Key · {count} 个模型</span></div>
          <span className={`${styles.dot} ${tone}`} aria-hidden="true" />
          <span className={styles.providerStatus}>{label}<small className="text-muted">{relativeTime(active?.lastVerifiedAt)}</small></span>
          <button className="text-button" onClick={() => onNavigate('providers')}>管理<ArrowRight size={14} /></button>
        </li>;
      })}</ul>
    </section>

    <section className={styles.card}>
      <div className={styles.cardHeader}><h2>待应用模型 <span className="badge">{pendingCount}</span></h2>
        <button className="text-button" onClick={() => onNavigate('models')}>查看全部<ArrowRight size={14} /></button></div>
      {pendingCount === 0
        ? <p className={styles.muted}><Boxes size={15} />没有待应用的改动。</p>
        : <ul className={styles.pending}>{models.filter(model => model.inCatalog && model.hostState !== 'loaded').slice(0, 5).map(model => <li key={model.id}>
          <span className="text-mono text-muted">{model.upstreamId}</span>
          <span className={styles.pendingName}>{model.displayName}</span>
          <span className="badge warning">待应用</span>
        </li>)}</ul>}
    </section>

    {pendingCount > 0 && <div className={styles.applyBar} role="region" aria-label="待应用的修改">
      <span><KeyRound size={16} />{pendingCount} 个模型待应用 · 目录改动需要 Codex 重新加载</span>
      <div className="actions">
        <button onClick={() => onNavigate('models')}><CircleHelp size={15} />去哪儿改</button>
        <button className="primary" onClick={() => onNavigate('codexConfig')}>查看差异并应用</button>
      </div>
    </div>}
  </div>;
}
