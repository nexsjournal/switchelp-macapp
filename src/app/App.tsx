import { useCallback, useEffect, useState } from 'react';
import { Activity, Boxes, ChevronRight, LayoutDashboard, ListChecks, Plus, RefreshCw, Search, Server, Settings2, ShieldCheck, SlidersHorizontal, Settings as SettingsIcon } from 'lucide-react';
import type { Credential, Model, Provider } from '@/contracts/types';
import { type AppliedSummary, type DesktopClient, type GatewayReport, type PlatformReport, toCoreError } from '@/desktop/client';
import { desktopClient } from '@/desktop/transport';
import { ProviderForm } from '@/features/providers/ProviderForm';
import { ModelEditorPage } from '@/features/models/ModelEditorPage';
import { OnboardingPage } from '@/features/onboarding/OnboardingPage';
import { OverviewPage } from '@/features/overview/OverviewPage';
import { ModelsPage } from '@/features/models/ModelsPage';
import { Dialog } from '@/components/Dialog';
import { ToastHost, showToast } from '@/components/Toast';
import { PendingApplyBar } from './PendingApplyBar';
import { EmptyState } from '@/components/EmptyState';
import { AppLogo } from '@/components/AppLogo';
import { CodexConfigPage } from '@/features/codex/CodexConfigPage';
import { ConnectionPage } from '@/features/diagnostics/ConnectionPage';
import { LogsPage } from '@/features/diagnostics/LogsPage';
import { SettingsPage } from '@/features/settings/SettingsPage';

import styles from './App.module.css';

import { useLocale, t } from '@/i18n';
type Page = 'overview' | 'providers' | 'codexConfig' | 'diagnostics' | 'logs' | 'settings';
const navigation = [
  { id: 'overview', icon: LayoutDashboard }, { id: 'providers', icon: Server },
  { id: 'codexConfig', icon: SlidersHorizontal },
  { id: 'diagnostics', icon: Activity }, { id: 'logs', icon: ListChecks },
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
  /** 首次接入向导：没有供应商时自动进入，用户可「稍后再说」并在设置里重新打开。 */
  const [onboardingDismissed, setOnboardingDismissed] = useState(() => {
    try { return localStorage.getItem('gptswitch.onboarding.dismissed') === 'true'; } catch { return false; }
  });
  const [onboardingForced, setOnboardingForced] = useState(false);
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
  const selectedProvider = providers.find(p => p.id === selectedProviderId);
  /** 没有供应商就是首次接入；用户主动关掉之后不再自动展开。 */
  const showOnboarding = (onboardingForced || (providers.length === 0 && !onboardingDismissed))
    && page === 'overview' && !modelEditor && !providerEditor;

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
   * 供应商弹窗保存成功后只刷新数据，**不关弹窗**：它就地变成这家供应商的编辑态，
   * 用户接着加 Key、获取模型，不必保存完再去找回来。
   * 提示由弹窗自己推（它知道这次保存是新键还是更新），这里不重复报一次。
   */
  function providerSaved() { keysChanged(); }
  const pending = models.filter(m => m.inCatalog && m.hostState !== 'loaded');
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
      <div className={styles.brand} data-tauri-drag-region="deep"><div className={styles.brandIcon}><AppLogo size={20} /></div><div><strong>{t('app.name')}</strong><span>{t('app.subtitle')}</span></div></div>
      <nav aria-label={t('shell.navLabel')}>{navigation.map(({ id, icon: Icon }) => <button key={id} className={page === id ? styles.active : ''} aria-current={page === id ? 'page' : undefined} onClick={() => navigate(id)}><Icon size={18} />{t(`nav.${id}`)}</button>)}</nav>
      {/* 设置按设计放在侧栏底部，与日常导航分开。 */}
      <button className={`${styles.settingsEntry} ${page === 'settings' ? styles.active : ''}`}
        aria-current={page === 'settings' ? 'page' : undefined} onClick={() => navigate('settings')}>
        <SettingsIcon size={18} />{t('nav.settings')}
      </button>
      <div className={styles.sidebarBottom}><ShieldCheck size={18} /><div><strong>{t('shell.localConfig')}</strong><span>{t('shell.credentialsInSecureStore')}</span></div></div>
      <div className={styles.version}>{t('app.name')} <span>{__APP_VERSION__} · {t('app.inDevelopment')}</span></div>
    </aside>
    <div className={styles.workspace}>
      <div className={styles.topbar} data-tauri-drag-region="deep"><span>{t('common.workspace')}<ChevronRight size={14} /> {t(`nav.${page}`)}</span>{gateway && !gateway.running ? <span className="badge warning">{t('overview.loadStateGatewayDown')}</span> : pending.length ? <span className="badge warning">{t('shell.pendingCountBadge', { count: pending.length })}</span> : gateway?.revisions.length ? <span className="badge">{t('shell.applied')}</span> : <span className="badge">{t('shell.notApplied')}</span>}</div>
      <main className={styles.main} id="main-content" tabIndex={-1}>
        {modelEditor ? <ModelEditorPage client={client} providers={providers}
          model={modelEditor === 'new' ? undefined : modelEditor}
          onDirtyChange={setEditorDirty}
          onCancel={() => { setModelEditor(null); setEditorDirty(false); }}
          onSaved={async () => { setModelEditor(null); setEditorDirty(false); showToast(t('copy.draftSaved')); await refresh(); }} /> : <>
        {!showOnboarding && <header className={styles.pageHeader}><div><h1 className="text-page-title">{t(`nav.${page}`)}</h1><p>{({ overview: t('page.overviewHint'), providers: t('page.providersHint'), codexConfig: t('page.codexHint'), diagnostics: t('page.diagnosticsHint'), logs: t('page.logsHint'), settings: t('page.settingsHint') })[page]}</p></div>
          <div className="actions"><button className="icon-button" aria-label={t('common.reload')} disabled={loading} onClick={() => void refresh()}><RefreshCw size={17} className={loading ? styles.spin : ''} /></button>
            {page !== 'codexConfig' && page !== 'logs' && page !== 'diagnostics' && page !== 'settings' && <button className="primary" disabled={loading} onClick={() => setProviderEditor('new')}><Plus size={17} />{t('action.addProvider')}</button>}</div></header>}
        {/* 页面状态（加载失败）留在页面里：它要一直看得见，直到状态本身改变。
            动作结果（已保存、已应用…）走全局 Toast，弹窗与常规界面共用同一个位置。 */}
        {error && <div className="error-message" role="alert">{error}</div>}
        {!loaded && !error ? <div className={styles.empty} role="status" aria-live="polite">{t('shell.loading')}</div> : <>
          {showOnboarding && <OnboardingPage client={client} providers={providers} models={models}
            credentialsByProvider={credentialsByProvider}
            onOpenProviderForm={() => setProviderEditor('new')}
            onOpenModelEditor={() => { if (providers.length) setModelEditor('new'); }}
            onViewDiff={() => navigate('codexConfig')}
            onDismiss={() => {
              setOnboardingForced(false);
              setOnboardingDismissed(true);
              try { localStorage.setItem('gptswitch.onboarding.dismissed', 'true'); } catch { /* 存不了只影响下次是否自动展开 */ }
            }} />}
          {page === 'overview' && !showOnboarding && <OverviewPage providers={providers} models={models}
            credentialsByProvider={credentialsByProvider} gateway={gateway} summary={summary}
            pendingCount={pending.length} onNavigate={navigate} onAddProvider={() => setProviderEditor('new')} />}
          {page === 'providers' && !providers.length && <section className={styles.card}>
            <EmptyState icon={Server} title={t('empty.addFirstProviderTitle')}
              description={t('providers.addFirstBody')}
              action={<button className="primary" onClick={() => setProviderEditor('new')}><Plus size={17} />{t('action.addProvider')}</button>} />
          </section>}
          {page === 'providers' && providers.length > 0 && <div className={styles.providerLayout}><section className={styles.providerList} aria-label={t('providers.list')}>
            {visibleProviders.map(provider => {
              const status = providerStatus(provider, credentialsByProvider[provider.id] ?? []);
              const modelCount = models.filter(m => m.providerId === provider.id).length;
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
            <div className={styles.cardHeader}><h3><Boxes size={17} />{t('models.title')}</h3>
              <button onClick={() => setModelEditor('new')} disabled={!selectedProvider}><Plus size={16} />{t('action.addModel')}</button></div>
            <ModelsPage client={client} providers={providers} models={models} onChanged={refresh}
              onEditModel={target => setModelEditor(target)} providerScope={selectedProvider.id} embedded />
          </section> : <section className={styles.card}><EmptyState icon={Settings2} title={t('providers.noneSelected')} description={t('providers.noneSelectedBody')} /></section>}</div>}
          {page === 'codexConfig' && <CodexConfigPage client={client} models={models} summary={summary} onApplied={() => void refresh()} />}
          {page === 'diagnostics' && <ConnectionPage client={client} providers={providers} />}
          {page === 'logs' && <LogsPage client={client} />}
          {page === 'settings' && <SettingsPage client={client} gateway={gateway} onNavigate={navigate} />}
          {/* 待应用条常驻在页面内容之后：配好供应商与模型之后，生效只差「应用 + 重启」这一步，
              入口不能只在 Codex 配置页里（真机上用户在自己配的页面上找不到任何能生效的按钮）。
              Codex 配置页自己就有完整的操作行，那里不重复出现。 */}
          {page !== 'codexConfig' && !showOnboarding && !modelEditor && <PendingApplyBar client={client} providers={providers}
            models={models} onApplied={refresh} onOpenDiff={() => navigate('codexConfig')} />}
        </>}
      </>}
      </main>
      <footer className={styles.statusbar}><span><span className={`${styles.dot} ${gateway?.running ? styles.online : styles.offline}`} />{gatewayText(gateway)}</span><span>{t('shell.pendingModels', { count: pending.length })} <span className={styles.separator}>/</span>{t('shell.configOnDevice')}</span></footer>
    </div>
    {providerEditor && <ProviderForm client={client} provider={providerEditor === 'new' ? undefined : providerEditor}
      providers={providers} models={models}
      onSaved={providerSaved} onKeysChanged={keysChanged} onChanged={refresh}
      onClose={() => setProviderEditor(null)} />}
    {pendingNav && <Dialog title={t('editor.discardTitle')} description={t('editor.discardBody')} dirty={false}
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

    {confirm && <Dialog title={confirm.title} description={confirm.body} onClose={() => setConfirm(null)} busy={confirmBusy}
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
