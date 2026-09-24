import { Fragment, useCallback, useEffect, useState } from 'react';
import { Activity, Boxes, ChevronRight, LayoutDashboard, ListChecks, Newspaper, PackageOpen, Plus, RefreshCw, Search, Server, Settings2, ShieldCheck, SlidersHorizontal, Wrench, Settings as SettingsIcon } from 'lucide-react';
import type { Credential, Model, Provider } from '@/contracts/types';
import { type AppliedSummary, type DesktopClient, type GatewayReport, type PlatformReport, type UpdateReport, toCoreError } from '@/desktop/client';
import { desktopClient } from '@/desktop/transport';
import { ProviderForm } from '@/features/providers/ProviderForm';
import { ModelEditorPage } from '@/features/models/ModelEditorPage';
import { OnboardingPage } from '@/features/onboarding/OnboardingPage';
import { OverviewPage } from '@/features/overview/OverviewPage';
import { ModelsPage } from '@/features/models/ModelsPage';
import { hostStateKeys, hostStateVariant, providerHostState } from '@/features/models/policy';
import { Dialog } from '@/components/Dialog';
import { ToastHost, showToast } from '@/components/Toast';
import { PendingApplyBar } from './PendingApplyBar';
import { EmptyState } from '@/components/EmptyState';
import { AppLogo } from '@/components/AppLogo';
import { CodexConfigPage } from '@/features/codex/CodexConfigPage';
import { ConnectionPage } from '@/features/diagnostics/ConnectionPage';
import { LogsPage } from '@/features/diagnostics/LogsPage';
import { SettingsPage } from '@/features/settings/SettingsPage';
import { ToolsPage } from '@/features/tools/ToolsPage';
import { PluginHubPage } from '@/features/plugins/PluginHubPage';
import { ContentPage } from '@/features/content/ContentPage';
import { UpdateDialog } from '@/features/update/UpdateDialog';
import { UpdatePill } from '@/features/update/UpdatePill';

import styles from './App.module.css';

import { useLocale, t } from '@/i18n';
type Page = 'overview' | 'providers' | 'codexConfig' | 'tools' | 'plugins' | 'content' | 'diagnostics' | 'logs' | 'settings';
/**
 * 侧栏导航。`group` 只做视觉分组：它是分隔标签，不可点击也不折叠——
 * 为一组分隔引入折叠状态，收益是一条线，成本是用户又要学一个新控件。
 */
const navigation = [
  { id: 'overview', icon: LayoutDashboard },
  { id: 'providers', icon: Server },
  { id: 'codexConfig', icon: SlidersHorizontal },
  // 「扩展」一组：内容中心放在最前（按用户要求），其余按「工具 → 插件」的因果顺序。
  { id: 'content', icon: Newspaper, group: 'shell.navGroup.extensions' },
  { id: 'tools', icon: Wrench },
  { id: 'plugins', icon: PackageOpen },
  { id: 'diagnostics', icon: Activity, group: 'shell.navGroup.diagnostics' },
  { id: 'logs', icon: ListChecks },
] as const;
/**
 * 网关状态文案。未启动时必须显示原因或“未启动”，绝不能笼统写成“已接通”。
 * `served` 是本进程已处理的推理请求数，用来区分“起来了但没人用”和“根本没起来”。
 */
function gatewayText(gateway: GatewayReport | null): string {
  if (!gateway) return t('shell.loadingGateway');
  if (!gateway.running) return gateway.error ? t('overview.gatewayDownDetail', { reason: gateway.error }) : t('overview.loadStateGatewayDown');
  const revisions = gateway.revisions.length;
  return t('overview.gatewayRunningDetail', {
    port: gateway.port ?? '—',
    revisions: revisions ? t('overview.catalogRevisions', { count: revisions }) : t('overview.noCatalogPublished'),
  });
}


/** 浏览器预览时的平台回落：Tauri 之外拿不到编译期结论。 */
function platformFromUserAgent(): PlatformReport {
  const agent = navigator.userAgent;
  if (/Windows/i.test(agent)) {
    return { platform: 'windows', titlebarHeight: 0, leadingReserve: 0, systemDecorations: true };
  }
  if (/Linux/i.test(agent)) {
    return { platform: 'linux', titlebarHeight: 0, leadingReserve: 0, systemDecorations: true };
  }
  return { platform: 'macos', titlebarHeight: 44, leadingReserve: 84, systemDecorations: true };
}

/** 把平台结论写到根元素：CSS 只认 `data-platform`，不猜 userAgent。 */
function applyPlatform(report: PlatformReport) {
  const root = document.documentElement;
  root.dataset.platform = report.platform;
  root.style.setProperty('--titlebar-height', `${report.titlebarHeight}px`);
  root.style.setProperty('--macos-traffic-light-reserve', `${report.leadingReserve}px`);
  // 系统绘制标题栏时界面不需要自绘拖拽区，避免出现两条标题栏。
  if (report.systemDecorations) root.dataset.systemDecorations = 'true';
  else delete root.dataset.systemDecorations;
}

/**
 * 供应商行的状态摘要。参考截图里每一行都能直接看到状态，而不是只有一个箭头；
 * 判定只依据本工具自己保存的 Key 元数据，不猜测连接是否可用。
 *
 * 这里只给状态，不给操作按钮：Key 与模型的增删改、测试连接都在供应商弹窗里，
 * 由「编辑配置」进入，行上不放第二套入口。
 */
function providerStatus(provider: Provider, credentials: Credential[]): { tone: 'success' | 'warning' | 'muted'; label: string } {
  if (!credentials.length) return { tone: 'muted', label: t('overview.keysMissing') };
  const active = credentials.find(credential => credential.id === provider.activeCredentialId);
  if (!active) return { tone: 'warning', label: t('overview.keysNoSelection', { count: credentials.length }) };
  if (active.status === 'verified') return { tone: 'success', label: t('overview.keyVerified', { label: active.label }) };
  if (active.status === 'auth_failed') return { tone: 'warning', label: t('overview.keyAuthFailed', { label: active.label }) };
  return { tone: 'muted', label: t('overview.keyUntested', { label: active.label }) };
}

export function App({ client = desktopClient, initialPage = 'overview' }: { client?: DesktopClient; initialPage?: Page }) {
  const [page, setPage] = useState<Page>(initialPage);
  /**
   * 订阅语言状态：切换语言必须重渲染整棵树，只更新设置页会让侧栏和导航留在旧语言。
   * `data-locale` 同时把解析后的语言暴露给 CSS，和 `data-platform` 一样。
   */
  const locale = useLocale();
  const [providers, setProviders] = useState<Provider[]>([]);
  const [models, setModels] = useState<Model[]>([]);
  /** 每个供应商各自的 Key 列表：供应商页每行都要显示状态，不能只加载当前选中的。 */
  const [credentialsByProvider, setCredentialsByProvider] = useState<Record<string, Credential[]>>({});
  const [gateway, setGateway] = useState<GatewayReport | null>(null);
  /** 当前已生效的配置；用于概览的“当前配置”卡。 */
  const [summary, setSummary] = useState<AppliedSummary | null>(null);
  /** 首次接入向导是否已被主动结束（「稍后再说」或走完最后一步），记住了就不再自动展开。 */
  const [onboardingDismissed, setOnboardingDismissed] = useState(() => {
    try { return localStorage.getItem('gptswitch.onboarding.dismissed') === 'true'; } catch { return false; }
  });
  /** 向导当前是否打开。**不能**由「零供应商」推导出来：那个推导会在保存第一个供应商的瞬间把向导关掉，
   *  于是第 3 步「测试并应用」永远走不到，主线就在半路断掉了。向导一旦打开就留到用户自己结束它。 */
  const [onboardingOpen, setOnboardingOpen] = useState(false);
  /**
   * 向导走到第几步。**放在宿主里**，与模型编辑器同源：向导在第 2 步会把人送去整页模型编辑器，
   * 那一刻向导会卸载；步骤留在组件内部的话，人一回来就又被扔回第 1 步。
   */
  const [onboardingStep, setOnboardingStep] = useState(0);
  const [selectedProviderId, setSelectedProviderId] = useState('');
  const [query, setQuery] = useState('');
  const [loading, setLoading] = useState(true);
  /** 首次加载完成前才整页占位；后续刷新不得卸载当前页面（会丢失事务流程状态）。 */
  const [loaded, setLoaded] = useState(false);
  const [keyVersion, setKeyVersion] = useState(0);
  const [error, setError] = useState('');
  const [providerEditor, setProviderEditor] = useState<Provider | 'new' | null>(null);
  /**
   * 模型编辑器全 App 只有一份状态：模型目录页、供应商卡片、接入向导都从这里打开。
   * 只有一个入口才能在侧栏导航时统一拦截脏表单，而不是各自为政地丢内容。
   * `'new'` 表示新建；`Model` 表示编辑既有模型。
   */
  const [modelEditor, setModelEditor] = useState<Model | 'new' | null>(null);
  /** 编辑器是否已有未保存内容；侧栏导航据此决定是直接离开还是先确认。 */
  const [editorDirty, setEditorDirty] = useState(false);
  /** 导航目标暂存：用户在确认框里选择放弃后，才真正离开编辑器。 */
  const [pendingNav, setPendingNav] = useState<Page | null>(null);
  /** 破坏性操作一律走确认：说明后果、说清不能撤销，再执行。 */
  const [confirm, setConfirm] = useState<{ title: string; body: string; confirmLabel: string; run: () => Promise<void> } | null>(null);
  const [confirmBusy, setConfirmBusy] = useState(false);
  /** 更新检查结果；失败或未检查时为 null（侧栏里就不会出现更新按钮）。 */
  const [update, setUpdate] = useState<UpdateReport | null>(null);
  const [updateOpen, setUpdateOpen] = useState(false);
  const selectedProvider = providers.find(p => p.id === selectedProviderId);
  /**
   * 向导渲染在概览页里。供应商弹窗是覆盖在上面的模态，**不**让向导卸载——否则用户点
   * 「添加供应商」再关掉弹窗，向导会退回第 1 步，白走一遍。整页模型编辑器是替换式渲染，
   * 那种情况下由 `onboardingStep` 记住位置。
   */
  const showOnboarding = onboardingOpen && page === 'overview' && !modelEditor;

  /** 结束向导：关掉并记住，之后不再自动展开（要再来一次得从设置页主动打开）。 */
  const closeOnboarding = useCallback(() => {
    setOnboardingOpen(false);
    setOnboardingDismissed(true);
    try { localStorage.setItem('gptswitch.onboarding.dismissed', 'true'); } catch { /* 存不了只影响下次是否自动展开 */ }
  }, []);
  /** 重新打开向导时从第 1 步开始：主动重来一遍的人要的是完整流程，不是上次停在哪。 */
  const openOnboarding = useCallback(() => {
    setOnboardingStep(0);
    setOnboardingOpen(true);
    setPage('overview');
  }, []);

  const refresh = useCallback(async () => {
    setLoading(true); setError('');
    try {
      const [providerResult, modelResult, gatewayResult, summaryResult] = await Promise.all([
        client.listProviders(), client.listModels(), client.gatewayStatus(), client.applySummary()]);
      setProviders(providerResult.items); setModels(modelResult); setGateway(gatewayResult); setSummary(summaryResult);
      setSelectedProviderId(current => providerResult.items.some(p => p.id === current) ? current : providerResult.items[0]?.id ?? '');
    } catch (e) { setError(toCoreError(e).safeDetails.join(t('common.listSeparator')) || t('shell.loadFailed')); }
    finally { setLoading(false); setLoaded(true); }
  }, [client]);
  useEffect(() => { void refresh(); }, [refresh]);
  /**
   * 宿主回执对账：宿主可能在我们不知情的时候重启过——用户自己重开了 Codex，或应用重启后
   * 再打开窗口。补记成功就重读数据，界面上的「等待重载」会自己消失。
   *
   * 失败不提示：这是后台对账，不是用户发起的动作；真失败时界面上仍留着「等待重载」与
   * 配置页那个手动确认入口，不会卡住。
   */
  const reconcileReload = useCallback(async () => {
    const result = await client.reconcileReload().catch(() => null);
    if (result?.confirmedOperationIds.length) await refresh();
  }, [client, refresh]);
  useEffect(() => { void reconcileReload(); }, [reconcileReload]);
  /**
   * 启动即查一次更新：更新按钮要在侧栏里自己出现，不能等用户先去设置页点「检查更新」。
   *
   * 失败静默——查不到就是没有按钮，不该在界面上报警（手动入口在设置页，那里会如实
   * 显示失败原因）。这里也不阻塞首屏：它跟数据加载并行跑。
   */
  useEffect(() => {
    void client.checkUpdate().then(setUpdate).catch(() => setUpdate(null));
  }, [client]);
  /**
   * 上一次更新装完的结果。安装成功时不会有收尾界面（装完即重启），所以「到底成功了没有」
   * 只能靠这条提示说清楚；标记文件读一次就删，只在更新后的第一次启动出现。
   */
  useEffect(() => {
    void client.takeUpdateResult()
      .then(version => { if (version) showToast(t('update.done', { version })); })
      .catch(() => undefined);
  }, [client]);
  useEffect(() => {
    // 窗口重新获得焦点是对账的最佳时机：用户切去重启 Codex、再切回来就走这条。
    window.addEventListener('focus', reconcileReload);
    return () => window.removeEventListener('focus', reconcileReload);
  }, [reconcileReload]);
  /**
   * 首次接入自动展开一次：还没有供应商，且用户没主动结束过向导。
   * 只在首屏加载完成后判断，避免数据还没到就先闪出一个向导。
   */
  useEffect(() => {
    if (loaded && !onboardingDismissed && providers.length === 0) setOnboardingOpen(true);
  }, [loaded, onboardingDismissed, providers.length]);
  useEffect(() => {
    let current = true;
    client.platformInfo()
      .then(info => { if (current) applyPlatform(info); })
      .catch(() => { if (current) applyPlatform(platformFromUserAgent()); });
    return () => { current = false; };
  }, [client]);
  useEffect(() => {
    let current = true;
    if (!providers.length) { setCredentialsByProvider({}); return; }
    Promise.all(providers.map(provider => client.listCredentials(provider.id)
      .then(items => [provider.id, items] as const)
      .catch(() => [provider.id, [] as Credential[]] as const)))
      .then(pairs => { if (current) setCredentialsByProvider(Object.fromEntries(pairs)); })
      .catch(e => { if (current) setError(toCoreError(e).safeDetails.join(t('common.listSeparator')) || t('shell.keysLoadFailed')); });
    return () => { current = false; };
  }, [client, providers, keyVersion]);

  function navigate(next: Page) {
    // 编辑器开着时侧栏导航不是死路：脏表单先确认，干净则直接走。
    // 以前 navigate 只改 page、从不清编辑器，面包屑说在别处、正文却还停在表单上。
    if (modelEditor) {
      if (editorDirty) { setPendingNav(next); return; }
      setModelEditor(null);
      setEditorDirty(false);
    }
    setPage(next); setQuery('');
  }
  /** Key 变了：供应商行的状态与详情里的当前 Key 都要跟着变。 */
  function keysChanged() { setKeyVersion(version => version + 1); void refresh(); }
  /**
   * 供应商弹窗保存成功后只刷新数据；**关不关弹窗由弹窗自己决定**（见 ProviderForm 的 save()：
   * 保存成功即关闭，因为用户把「不关」读成「没保存成功」）。
   * 提示也由弹窗自己推（它知道这次保存是新键还是更新），这里不重复报一次。
   */
  function providerSaved() { keysChanged(); }
  const pending = models.filter(m => m.inCatalog && m.hostState !== 'loaded');
  /** 待办只剩「等宿主回执」时，底栏改说事实：已经应用了，只是还没确认 Codex 读过。 */
  const awaitingHostOnly = pending.length > 0 && pending.every(m => m.hostState === 'awaiting_reload');
  const search = query.trim().toLocaleLowerCase();
  const visibleProviders = providers.filter(p => `${p.name} ${p.endpoint}`.toLocaleLowerCase().includes(search));

  return <div className={styles.shell} data-locale={locale}>
    {/*
      透明的窗口拖拽区。
      macOS 的标题栏是 overlay（透明），鼠标事件全部由 webview 接收——没有这一条，窗口就
      没法用鼠标移动了。它不画任何东西：露出来的就是侧栏与顶栏自己的主题色，所以不存在
      配色问题。高度取 --titlebar-height（非 macOS 平台是 0，等于不存在）。
    */}
    <div className={styles.dragRegion} data-tauri-drag-region aria-hidden="true" />
    {/* 显式把焦点交给主内容：部分引擎不会为片段链接移动焦点。 */}
    <a className="skip-link" href="#main-content" onClick={() => document.getElementById('main-content')?.focus()}>{t('common.skipToContent')}</a>
    <aside className={styles.sidebar}>
      <div className={styles.brand} data-tauri-drag-region="deep">
        <div className={styles.brandIcon}><AppLogo size={20} /></div>
        <div>
          <strong>{t('app.name')}</strong>
          {/* 有更新时按钮**顶掉副标题那一行**（同一位置、同一列），所以品牌块只有两行，
              徽标也仍然是和这一整列居中的——把按钮挂到这一列外面，徽标就只跟名字那一行居中，
              看起来「徽标和按钮没对齐」。 */}
          {update?.hasUpdate && update.latest
            ? <span className={styles.brandUpdate}><UpdatePill version={update.latest} onClick={() => setUpdateOpen(true)} /></span>
            : <span className={styles.brandSubtitle}>{t('app.subtitle')}</span>}
        </div>
      </div>
      {/* 文字包在 span 里：窄窗（含 200% 缩放）侧栏收窄成图标栏时把它视觉隐藏，
          但保留在无障碍树里——收起文字不该把按钮的可见名字也一起收掉。 */}
      <nav aria-label={t('shell.navLabel')}>{navigation.map(({ id, icon: Icon, ...entry }) => <Fragment key={id}>
        {/* 分组标签只在组的第一个条目前出现一次。 */}
        {'group' in entry && entry.group && <span className={styles.navGroup} aria-hidden="true">{t(entry.group)}</span>}
        <button className={page === id ? styles.active : ''} aria-current={page === id ? 'page' : undefined} onClick={() => navigate(id)}><Icon size={18} /><span className={styles.navLabel}>{t(`nav.${id}`)}</span></button>
      </Fragment>)}</nav>
      {/* 设置按设计放在侧栏底部，与日常导航分开。 */}
      <button className={`${styles.settingsEntry} ${page === 'settings' ? styles.active : ''}`}
        aria-current={page === 'settings' ? 'page' : undefined} onClick={() => navigate('settings')}>
        <SettingsIcon size={18} /><span className={styles.navLabel}>{t('nav.settings')}</span>
      </button>
      <div className={styles.sidebarBottom}><ShieldCheck size={18} /><div><strong>{t('shell.localConfig')}</strong><span>{t('shell.credentialsInSecureStore')}</span></div></div>
      <div className={styles.version}>{t('app.name')} <span>{__APP_VERSION__}</span></div>
    </aside>
    <div className={styles.workspace}>
      <div className={styles.topbar} data-tauri-drag-region="deep"><span>{t('common.workspace')}<ChevronRight size={14} /> {t(`nav.${page}`)}</span>{gateway && !gateway.running ? <span className="badge warning">{t('overview.loadStateGatewayDown')}</span> : pending.length ? <span className="badge warning">{awaitingHostOnly ? t('shell.awaitingHostBadge', { count: pending.length }) : t('shell.pendingCountBadge', { count: pending.length })}</span> : gateway?.revisions.length ? <span className="badge">{t('shell.applied')}</span> : <span className="badge">{t('shell.notApplied')}</span>}</div>
      <main className={styles.main} id="main-content" tabIndex={-1}>
        {modelEditor ? <ModelEditorPage client={client} providers={providers}
          model={modelEditor === 'new' ? undefined : modelEditor}
          onDirtyChange={setEditorDirty}
          onCancel={() => { setModelEditor(null); setEditorDirty(false); }}
          onSaved={async () => { setModelEditor(null); setEditorDirty(false); showToast(t('copy.draftSaved')); await refresh(); }} /> : <>
        {!showOnboarding && <header className={styles.pageHeader}><div><h1 className="text-page-title">{t(`nav.${page}`)}</h1><p>{({
              overview: t('page.overviewHint'), providers: t('page.providersHint'), codexConfig: t('page.codexHint'),
              tools: t('page.toolsHint'), plugins: t('page.pluginsHint'), content: t('page.contentHint'),
              diagnostics: t('page.diagnosticsHint'), logs: t('page.logsHint'), settings: t('page.settingsHint'),
            })[page]}</p></div>
          <div className="actions"><button className="icon-button" aria-label={t('common.reload')} disabled={loading} onClick={() => void refresh()}><RefreshCw size={18} className={loading ? styles.spin : ''} /></button>
            {page !== 'codexConfig' && page !== 'logs' && page !== 'diagnostics' && page !== 'settings'
              && page !== 'tools' && page !== 'plugins' && page !== 'content'
              && <button className="primary" disabled={loading} onClick={() => setProviderEditor('new')}><Plus size={18} />{t('action.addProvider')}</button>}</div></header>}
        {/* 页面状态（加载失败）留在页面里：它要一直看得见，直到状态本身改变。
            动作结果（已保存、已应用…）走全局 Toast，弹窗与常规界面共用同一个位置。 */}
        {error && <div className="error-message" role="alert">{error}</div>}
        {/*
          * 「应用 + 重启」只出现在供应商与模型页，紧跟页头——用户正是在这一页配完供应商与模型，
          * 生效的入口就该在这一页看得见。它以前是**每个页面**吸底的一条，理由是「入口不能只在
          * Codex 配置页里」，但代价是任何一页都被它盖住一行、看起来像全局状态栏。
          * 「有几个模型待确认加载」这个事实在上方胶囊与底栏仍然可见，口径是同一句文案。
          */}
        {page === 'providers' && !showOnboarding && !modelEditor && <PendingApplyBar client={client} providers={providers}
          models={models} summary={summary} onApplied={refresh} onOpenDiff={() => navigate('codexConfig')} />}
        {!loaded && !error ? <div className={styles.empty} role="status" aria-live="polite">{t('shell.loading')}</div> : <>
          {showOnboarding && <OnboardingPage client={client} providers={providers} models={models}
            credentialsByProvider={credentialsByProvider}
            step={onboardingStep} onStepChange={setOnboardingStep}
            onOpenProviderForm={() => setProviderEditor('new')}
            onOpenModelEditor={() => { if (providers.length) setModelEditor('new'); }}
            /* 走到最后一步就是「去应用」：向导的活干完了，不再留在概览页上。 */
            onViewDiff={() => { closeOnboarding(); navigate('codexConfig'); }}
            onDismiss={closeOnboarding} />}
          {page === 'overview' && !showOnboarding && <OverviewPage providers={providers} models={models}
            credentialsByProvider={credentialsByProvider} gateway={gateway} summary={summary}
            pendingCount={pending.length} awaitingHostOnly={awaitingHostOnly} onNavigate={navigate} onAddProvider={() => setProviderEditor('new')} />}
          {page === 'providers' && !providers.length && <section className={styles.card}>
            <EmptyState icon={Server} title={t('empty.addFirstProviderTitle')}
              description={t('providers.addFirstBody')}
              action={<button className="primary" onClick={() => setProviderEditor('new')}><Plus size={18} />{t('action.addProvider')}</button>} />
          </section>}
          {page === 'providers' && providers.length > 0 && <div className={styles.providerLayout}><section className={styles.providerList} aria-label={t('providers.list')}>
            {/* 搜索框以前缺着：`query` 状态、过滤逻辑与「没有匹配」的空态都在，就是没有输入的地方——
                供应商一多只能靠眼睛在 300px 的列表里找。 */}
            {providers.length > 1 && <div className={styles.providerSearch}>
              <Search size={16} aria-hidden="true" />
              <input type="search" value={query} aria-label={t('providers.searchAria')} placeholder={t('providers.searchPlaceholder')}
                onChange={event => setQuery(event.target.value)} />
            </div>}
            {visibleProviders.map(provider => {
              const own = models.filter(m => m.providerId === provider.id);
              const modelCount = own.length;
              /*
               * 「已加载」是**供应商这一层**的事实：同一家的模型要么一起进了菜单、要么一起
               * 待应用。逐行显示既啰嗦又容易被读成「每个模型各自的状态」。
               * 一家供应商一个模型都没配时，Codex 状态无从谈起，那一行退回 Key 的状态。
               */
              const hostState = providerHostState(own);
              const status = hostState
                ? { tone: (hostStateVariant(hostState) || 'muted') as 'success' | 'warning' | 'muted', label: t(hostStateKeys[hostState]) }
                : providerStatus(provider, credentialsByProvider[provider.id] ?? []);
              const selected = selectedProviderId === provider.id;
              return <button key={provider.id} className={`${styles.providerItem} ${selected ? styles.selected : ''}`}
                onClick={() => setSelectedProviderId(provider.id)} aria-current={selected ? 'true' : undefined}>
                <span className={styles.providerHead}>
                  <span className={styles.monogram}>{provider.name.slice(0, 1)}</span>
                  <span className={styles.providerName}><strong>{provider.name}</strong><span>{provider.enabled ? t('providers.modelCount', { count: modelCount }) : t('providers.disabledModelCount', { count: modelCount })}</span></span>
                </span>
                <span className={styles.providerStatus}>
                  <span className={`${styles.statusDot} ${styles[status.tone]}`} aria-hidden="true" />
                  <span className={styles.statusLabel}>{status.label}</span>
                </span>
              </button>;
            })}
            {!visibleProviders.length && <EmptyState icon={Search} title={t('providers.noMatch')} description={t('providers.noMatchBody')} />}
          </section>{selectedProvider ? <section className={styles.card}>
            <div className={styles.cardHeader}><h2>{selectedProvider.name}</h2><div className="actions">
              <button onClick={() => setProviderEditor(selectedProvider)}>{t('providers.editConfig')}</button>
              <button className="danger" aria-label={t('providers.deleteProviderAria', { name: selectedProvider.name })}
                onClick={() => setConfirm({
                  title: t('providers.deleteProvider'),
                  body: t('providers.deleteProviderBody', {
                    name: selectedProvider.name,
                    keys: (credentialsByProvider[selectedProvider.id] ?? []).length,
                    models: models.filter(model => model.providerId === selectedProvider.id).length,
                  }),
                  confirmLabel: t('providers.deleteProvider'),
                  run: async () => { await client.deleteProvider(selectedProvider.id); },
                })}>{t('providers.deleteProvider')}</button>
            </div></div>
            <dl className={styles.details}><dt>{t('providers.endpoint')}</dt><dd className="text-mono break-anywhere">{selectedProvider.endpoint}</dd><dt>{t('providers.protocol')}</dt><dd>{selectedProvider.protocol === 'responses' ? 'Responses' : t('providers.chatPending')}</dd><dt>{t('providers.authKind')}</dt><dd>{selectedProvider.authKind === 'api_key' ? t('auth.apiKey') : t('providers.noAuth')}</dd></dl>
            {/*
              这一页只放事实（地址、协议、模型）与动作入口：Key 的增删改、获取模型、
              测试连接都在供应商弹窗里，由「编辑配置」进入。同一件事只有一个入口，
              才不会出现两处谁才算数的问题。
            */}
            <div className={styles.cardHeader}><h3><Boxes size={18} />{t('models.title')}</h3>
              <button onClick={() => setModelEditor('new')} disabled={!selectedProvider}><Plus size={16} />{t('action.addModel')}</button></div>
            <ModelsPage client={client} providers={providers} models={models} onChanged={refresh}
              onEditModel={target => setModelEditor(target)} providerScope={selectedProvider.id} embedded />
          </section> : <section className={styles.card}><EmptyState icon={Settings2} title={t('providers.noneSelected')} description={t('providers.noneSelectedBody')} /></section>}</div>}
          {page === 'codexConfig' && <CodexConfigPage client={client} models={models} summary={summary} onApplied={() => void refresh()} />}
          {page === 'tools' && <ToolsPage client={client} />}
          {page === 'plugins' && <PluginHubPage client={client} />}
          {page === 'content' && <ContentPage client={client} />}
          {page === 'diagnostics' && <ConnectionPage client={client} providers={providers} />}
          {page === 'logs' && <LogsPage client={client} />}
          {page === 'settings' && <SettingsPage client={client} gateway={gateway} onNavigate={navigate}
            onReopenOnboarding={openOnboarding} />}
        </>}
      </>}
      </main>
      <footer className={styles.statusbar}><span><span className={`${styles.dot} ${gateway?.running ? styles.online : styles.offline}`} />{gatewayText(gateway)}</span><span>{awaitingHostOnly ? t('shell.awaitingHost', { count: pending.length }) : t('shell.pendingModels', { count: pending.length })} <span className={styles.separator}>/</span>{t('shell.configOnDevice')}</span></footer>
    </div>
    {providerEditor && <ProviderForm client={client} provider={providerEditor === 'new' ? undefined : providerEditor}
      providers={providers} models={models}
      onSaved={providerSaved} onKeysChanged={keysChanged} onChanged={refresh}
      onClose={() => setProviderEditor(null)} />}
    {pendingNav && <Dialog width="narrow" title={t('editor.discardTitle')} description={t('editor.discardBody')} dirty={false}
      onClose={() => setPendingNav(null)} footer={<footer className="form-footer">
        <span>{t('editor.discardIrreversible')}</span>
        <div className="actions">
          <button onClick={() => setPendingNav(null)} autoFocus>{t('editor.keepEditing')}</button>
          <button className="danger" onClick={() => {
            const target = pendingNav;
            setPendingNav(null);
            setModelEditor(null);
            setEditorDirty(false);
            setPage(target); setQuery('');
          }}>{t('editor.leaveDiscard')}</button>
        </div>
      </footer>} />}
    {/* 唯一的提示宿主：不论从哪个页面、哪个弹窗推的提示，都出现在同一个位置。 */}
    <ToastHost />

    {/* 更新弹窗：入口在侧栏左上角的更新胶囊。 */}
    {updateOpen && update && <UpdateDialog client={client} report={update} onClose={() => setUpdateOpen(false)} />}

    {confirm && <Dialog width="narrow" title={confirm.title} description={confirm.body} onClose={() => setConfirm(null)} busy={confirmBusy}
      footer={<footer className="form-footer">
          <span>{t('common.irreversible')}</span>
          <div className="actions">
            <button onClick={() => setConfirm(null)} disabled={confirmBusy}>{t('action.cancel')}</button>
            <button className="danger" disabled={confirmBusy} autoFocus onClick={async () => {
              setConfirmBusy(true); setError('');
              try {
                await confirm.run();
                showToast(t('common.done'));
                setConfirm(null);
                await refresh();
              } catch (e) { setError(toCoreError(e).safeDetails.join(t('common.listSeparator')) || t('common.failed')); }
              finally { setConfirmBusy(false); }
            }}>{confirmBusy ? t('common.busy') : confirm.confirmLabel}</button>
          </div>
      </footer>} />}
  </div>;
}
