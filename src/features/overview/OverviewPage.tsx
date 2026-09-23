import { useMemo } from 'react';
import { Activity, ArrowRight, Boxes, Circle, CircleCheck, CircleHelp, KeyRound, Plus, Server, Settings2, ShieldCheck } from 'lucide-react';
import type { Credential, Model, Provider } from '@/contracts/types';
import type { AppliedSummary, GatewayReport } from '@/desktop/client';
import { EmptyState } from '@/components/EmptyState';
import { hostStateKeys, hostStateVariant } from '@/features/models/policy';
import styles from './OverviewPage.module.css';

import { t } from '@/i18n';
/**
 * 概览页（设计 P02）。
 *
 * 两条原则写进实现：
 * - **不谎称**：当前配置卡显示的是本工具**已发布**的默认路由，不声称这是 Codex 当前每个会话在用的模型；
 *   “模型调用”在没有测试记录时显示未测试，而不是显示通过。
 * - **不展示没有可靠来源的数字**：不出现余额、成功率、Token 节省这类估算。
 */

function relativeTime(value: string | null | undefined): string {
  if (!value) return t('time.never');
  const stamp = Date.parse(value);
  if (Number.isNaN(stamp)) return t('time.never');
  const minutes = Math.floor((Date.now() - stamp) / 60_000);
  if (minutes < 1) return t('time.justNow');
  if (minutes < 60) return t('time.minutesAgo', { count: minutes });
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return t('time.hoursAgo', { count: hours });
  return t('time.daysAgo', { count: Math.floor(hours / 24) });
}

/** Codex 侧的加载状态：只由事务阶段推断，不猜宿主行为。 */
function codexState(summary: AppliedSummary | null, gateway: GatewayReport | null): { text: string; tone: 'ok' | 'warn' | 'muted' } {
  if (!gateway?.running) return { text: t('overview.loadStateGatewayDown'), tone: 'warn' };
  if (!summary) return { text: t('overview.loadStateNotApplied'), tone: 'muted' };
  // 阶段由 Rust 序列化成 snake_case：写成 'Verified' 这个分支永远不会命中，
  // 已核验的配置会被显示成「等待重新加载」。类型上也已收紧，写错编译不过。
  if (summary.stage === 'verified') return { text: t('overview.loadStateVerified'), tone: 'ok' };
  if (summary.stage === 'pending') return { text: t('overview.loadStatePending'), tone: 'warn' };
  return { text: t('codex.awaitingReload'), tone: 'warn' };
}

export function OverviewPage({ providers, models, credentialsByProvider, gateway, summary, pendingCount, awaitingHostOnly, onNavigate, onAddProvider }: {
  providers: Provider[];
  models: Model[];
  credentialsByProvider: Record<string, Credential[]>;
  gateway: GatewayReport | null;
  summary: AppliedSummary | null;
  pendingCount: number;
  /** 待办只剩「等宿主回执」：已提交但 Codex 还没确认加载，此时不该再说「待应用」。 */
  awaitingHostOnly: boolean;
  onNavigate: (page: 'providers' | 'codexConfig' | 'diagnostics' | 'settings') => void;
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
  /** 系统代理的观察结果；网关报告还没回来时为 null（那一条就不显示，不猜）。 */
  const proxy = gateway?.systemProxy ?? null;

  /** 每一步的完成判据都必须是可核实的观察值，不是估算：没有「大概完成」这种状态。 */
  const providersWithKey = providers.filter(provider => (credentialsByProvider[provider.id] ?? []).length > 0).length;
  const catalogCount = models.filter(model => model.inCatalog).length;
  const applyDetail = summary?.stage === 'verified' ? t('overview.stepApplyVerified')
    : pendingCount > 0 ? t('overview.stepApplyPending', { count: pendingCount })
    : t('overview.stepApplyNone');
  const steps: { titleKey: string; detail: string; done: boolean; page: 'providers' | 'codexConfig' | 'settings' }[] = [
    { titleKey: 'overview.stepProviders', done: providers.length > 0, page: 'providers',
      detail: providers.length ? t('overview.stepProvidersDetail', { count: providers.length }) : t('overview.stepProvidersEmpty') },
    // 有 Key 不等于验证过：这一步只说「填了」，验证结果在下面供应商那一栏里。
    { titleKey: 'overview.stepKeys', done: providers.length > 0 && providersWithKey === providers.length, page: 'providers',
      detail: t('overview.stepKeysDetail', { ready: providersWithKey, total: providers.length }) },
    { titleKey: 'overview.stepModels', done: models.length > 0, page: 'providers',
      detail: models.length ? t('overview.stepModelsDetail', { count: models.length, catalog: catalogCount }) : t('overview.stepModelsEmpty') },
    { titleKey: 'overview.stepApply', done: summary?.stage === 'verified', page: 'codexConfig', detail: applyDetail },
    { titleKey: 'overview.stepGateway', done: Boolean(gateway?.running), page: 'settings',
      detail: gateway?.running ? t('overview.stepGatewayRunning', { port: gateway.port ?? '—' }) : t('overview.stepGatewayStopped') },
  ];

  if (!providers.length) {
    return <section className={styles.card}><EmptyState icon={Server} title={t('empty.addFirstProviderTitle')}
      description={t('overview.addFirstProviderBody')}
      action={<button className="primary" onClick={onAddProvider}><Plus size={18} />{t('action.addProvider')}</button>} /></section>;
  }

  return <div className={styles.page}>
    {/*
     * 接入进度：这一页最先要回答的是「我弄了哪些、还差哪些」。
     * 五步各自带一个可核实的判据（供应商数量、Key、模型、最近一次成功应用、网关进程），
     * 不是估算出来的进度条。未完成的那一步整块可点，直接跳到能处理它的页面。
     */}
    <section className={styles.card}>
      <div className={styles.cardHeader}>
        <h2>{t('overview.readiness')}</h2>
        <span className="text-muted">{t('overview.readinessSummary', { done: steps.filter(step => step.done).length, total: steps.length })}</span>
      </div>
      <ol className={styles.steps}>
        {steps.map(step => {
          const Inner = <>
            <span className={`${styles.stepMark} ${step.done ? styles.stepDone : styles.stepTodo}`} aria-hidden="true">
              {step.done ? <CircleCheck size={16} /> : <Circle size={16} />}
            </span>
            <span className={styles.stepTitle}>{t(step.titleKey)}</span>
            <span className={styles.stepDetail}>{step.detail}</span>
          </>;
          return <li key={step.titleKey}>
            {step.done
              ? <div className={styles.step}>{Inner}</div>
              : <button type="button" className={styles.step} aria-label={t('overview.stepTodo', { step: t(step.titleKey) })}
                  onClick={() => onNavigate(step.page)}>{Inner}</button>}
          </li>;
        })}
      </ol>
    </section>

    <div className={styles.grid}>
      <section className={styles.card}>
        <div className={styles.cardHeader}><h2>{t('codex.inspectTitle')}</h2>
          {summary && <span className="badge">{summary.catalogRevision}</span>}
        </div>
        {routing ? <>
          <p className={styles.route}>
            <strong>{routing.model.displayName}</strong>
            <span className="text-muted">{routing.provider?.name ?? t('common.unknownProvider')} · {routing.model.upstreamId}</span>
          </p>
          <dl className={styles.rows}>
            <dt>{t('overview.subsequentRequests')}</dt>
            <dd>{routing.active ? `${routing.active.label} ${routing.active.maskedSuffix}` : t('overview.keyNotSelected')}</dd>
            <dt>{t('overview.catalogModels')}</dt>
            <dd>{t('overview.publishedCount', { count: summary?.aliasCount ?? 0 })}</dd>
          </dl>
        </> : <p className={styles.muted}>
          {t('overview.noAppliedConfig', { count: pendingCount })}
        </p>}
        <div className={styles.cardActions}>
          <button onClick={() => onNavigate('providers')}>{t('action.viewModels')}</button>
          <button className="primary" onClick={() => onNavigate('codexConfig')}>{t('nav.codexConfig')}</button>
        </div>
        {/* 外层 flex 只放图标与文字块；文字必须包在同一个子元素里，
            否则文字块会自己成为 flex 项而被块化、排版打散。 */}
        <div className={styles.note}><ShieldCheck size={14} />
          <p>{t('overview.configBoundary')}</p></div>
      </section>

      <section className={styles.card}>
        <div className={styles.cardHeader}><h2>{t('overview.connectionStatus')}</h2><Activity size={18} /></div>
        <ul className={styles.status}>
          <li>
            <span className={`${styles.dot} ${gateway?.running ? styles.ok : styles.warn}`} aria-hidden="true" />
            <div><strong>{t('overview.localService')}</strong><span>{gateway?.running ? t('overview.gatewayRunningShort', { port: gateway.port ?? '—' }) : t('overview.gatewayStopped')}</span></div>
          </li>
          <li>
            <span className={`${styles.dot} ${styles.muted}`} aria-hidden="true" />
            <div><strong>{t('overview.modelCall')}</strong><span>{t('overview.untested')}<span className="text-muted">{t('overview.runReadOnlyCheck')}</span></span></div>
          </li>
          <li>
            <span className={`${styles.dot} ${loading.tone === 'ok' ? styles.ok : loading.tone === 'warn' ? styles.warn : styles.muted}`} aria-hidden="true" />
            <div><strong>{t('overview.codexLoad')}</strong><span>{loading.text}</span></div>
          </li>
          {/*
            系统代理这一条只在拿得到观察结果时出现（网关报告还没回来时不说）。
            它回答的是「Codex 为什么报 502」：宿主到本机网关是回环地址，而它自己的
            HTTP 客户端会把系统代理套上去，代理又到不了 127.0.0.1。措辞只讲**机制与
            我们做过什么**，不断言「你现在正失败」——用户可能已经重启过 Codex 了。
          */}
          {proxy && <li>
            <span className={`${styles.dot} ${proxy.httpEnabled ? styles.warn : styles.muted}`} aria-hidden="true" />
            <div><strong>{t('overview.systemProxy')}</strong>
              <span>{!proxy.httpEnabled
                ? t('overview.systemProxyOff')
                : t(proxy.bypassApplied ? 'overview.systemProxyBypassed' : 'overview.systemProxyNeedsRestart', { endpoint: proxy.endpoint ?? '—' })}</span></div>
          </li>}
        </ul>
        {gateway?.error && <p className={styles.gatewayError} role="alert">{gateway.error}</p>}
        <div className={styles.cardActions}>
          <button onClick={() => onNavigate('diagnostics')}>{t('nav.diagnostics')}</button>
          <button onClick={() => onNavigate('settings')}><Settings2 size={16} />{t('overview.gatewaySettings')}</button>
        </div>
      </section>
    </div>

    <section className={styles.card}>
      <div className={styles.cardHeader}>
        <h2>{t('overview.providers')}</h2>
        <span className="text-muted">{t('overview.lastChecked', { time: relativeTime(recentDetection) })}</span>
      </div>
      <ul className={styles.providers}>{providers.slice(0, 5).map(provider => {
        const credentials = credentialsByProvider[provider.id] ?? [];
        const active = credentials.find(item => item.id === provider.activeCredentialId);
        const count = models.filter(model => model.providerId === provider.id).length;
        const tone = !credentials.length ? styles.muted : active ? styles.ok : styles.warn;
        const label = !credentials.length ? t('overview.keysMissing') : active
          ? (active.status === 'verified' ? t('overview.keyVerified', { label: active.label }) : t('overview.keyUntested', { label: active.label }))
          : t('overview.keyNotSelected');
        return <li key={provider.id}>
          <div className={styles.monogram}>{provider.name.slice(0, 1)}</div>
          <div className={styles.providerName}><strong>{provider.name}</strong>
            <span>{t('overview.keyAndModelCount', { keys: credentials.length, models: count })}</span></div>
          <span className={`${styles.dot} ${tone}`} aria-hidden="true" />
          <span className={styles.providerStatus}>{label}<small className="text-muted">{relativeTime(active?.lastVerifiedAt)}</small></span>
          <button className="text-button" onClick={() => onNavigate('providers')}>{t('overview.manage')}<ArrowRight size={14} /></button>
        </li>;
      })}</ul>
    </section>

    <section className={styles.card}>
      <div className={styles.cardHeader}><h2>{awaitingHostOnly ? t('overview.awaitingHostModels') : t('overview.pendingModels')}<span className="badge">{pendingCount}</span></h2>
        <button className="text-button" onClick={() => onNavigate('providers')}>{t('overview.viewAll')}<ArrowRight size={14} /></button></div>
      {pendingCount === 0
        ? <p className={styles.muted}><Boxes size={14} />{t('overview.nothingPending')}</p>
        : <ul className={styles.pending}>{models.filter(model => model.inCatalog && model.hostState !== 'loaded').slice(0, 5).map(model => <li key={model.id}>
          <span className="text-mono text-muted">{model.upstreamId}</span>
          <span className={styles.pendingName}>{model.displayName}</span>
          <span className={`badge ${hostStateVariant(model.hostState)}`}>{t(hostStateKeys[model.hostState])}</span>
        </li>)}</ul>}
    </section>

    {pendingCount > 0 && <div className={styles.applyBar} role="region" aria-label={awaitingHostOnly ? t('overview.awaitingHostChanges') : t('overview.pendingChanges')}>
      <span><KeyRound size={16} />{awaitingHostOnly ? t('overview.awaitingHostBar', { count: pendingCount }) : t('overview.pendingBar', { count: pendingCount })}</span>
      <div className="actions">
        <button onClick={() => onNavigate('providers')}><CircleHelp size={14} />{t('overview.whereToChange')}</button>
        <button className="primary" onClick={() => onNavigate('codexConfig')}>{awaitingHostOnly ? t('overview.confirmHostLoading') : t('overview.viewDiffAndApply')}</button>
      </div>
    </div>}
  </div>;
}
