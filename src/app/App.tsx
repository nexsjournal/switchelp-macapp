import { useCallback, useEffect, useState } from 'react';
import { Activity, ArrowLeftRight, Boxes, Check, ChevronRight, CircleHelp, KeyRound, LayoutDashboard, Plus, RefreshCw, Search, Server, Settings2, ShieldCheck, SlidersHorizontal } from 'lucide-react';
import type { Credential, Model, Provider } from '@/contracts/types';
import { type DesktopClient, type GatewayReport, type PlatformReport, toCoreError } from '@/desktop/client';
import { desktopClient } from '@/desktop/transport';
import { ProviderForm } from '@/features/providers/ProviderForm';
import { CredentialForm } from '@/features/providers/CredentialForm';
import { ModelForm } from '@/features/models/ModelForm';
import { CodexConfigPage } from '@/features/codex/CodexConfigPage';
import { ProbePanel } from '@/features/providers/ProbePanel';
import { DiagnosticsPage } from '@/features/diagnostics/DiagnosticsPage';
import { t } from '@/locales/zh-CN';
import styles from './App.module.css';

type Page = 'overview' | 'providers' | 'models' | 'codexConfig' | 'diagnostics';
const navigation = [
  { id: 'overview', icon: LayoutDashboard }, { id: 'providers', icon: Server },
  { id: 'models', icon: Boxes }, { id: 'codexConfig', icon: SlidersHorizontal },
  { id: 'diagnostics', icon: Activity },
] as const;
/**
 * 网关状态文案。未启动时必须显示原因或“未启动”，绝不能笼统写成“已接通”。
 * `served` 是本进程已处理的推理请求数，用来区分“起来了但没人用”和“根本没起来”。
 */
function gatewayText(gateway: GatewayReport | null): string {
  if (!gateway) return '正在读取网关状态…';
  if (!gateway.running) return gateway.error ? `网关未启动：${gateway.error}` : '网关未启动';
  const revisions = gateway.revisions.length;
  return `网关运行中 · 127.0.0.1:${gateway.port} · ${revisions ? `${revisions} 个目录版本` : '尚未发布目录'}`;
}

const hostLabels: Record<Model['hostState'], string> = { not_in_catalog: '未加入目录', pending_apply: '待应用', awaiting_reload: '等待重载', loaded: '已加载', load_unconfirmed: '加载未确认' };

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
 */
function providerStatus(provider: Provider, credentials: Credential[]): { tone: 'success' | 'warning' | 'muted'; label: string; action: string } {
  if (!credentials.length) return { tone: 'muted', label: '待填写 Key', action: '配置' };
  const active = credentials.find(credential => credential.id === provider.activeCredentialId);
  if (!active) return { tone: 'warning', label: `${credentials.length} 个 Key · 未选择`, action: '管理' };
  if (active.status === 'verified') return { tone: 'success', label: `${active.label} · 已验证`, action: '管理' };
  if (active.status === 'auth_failed') return { tone: 'warning', label: `${active.label} · 认证失败`, action: '管理' };
  return { tone: 'muted', label: `${active.label} · 未检测`, action: '管理' };
}

export function App({ client = desktopClient, initialPage = 'overview' }: { client?: DesktopClient; initialPage?: Page }) {
  const [page, setPage] = useState<Page>(initialPage);
  const [providers, setProviders] = useState<Provider[]>([]);
  const [models, setModels] = useState<Model[]>([]);
  /** 每个供应商各自的 Key 列表：供应商页每行都要显示状态，不能只加载当前选中的。 */
  const [credentialsByProvider, setCredentialsByProvider] = useState<Record<string, Credential[]>>({});
  const [gateway, setGateway] = useState<GatewayReport | null>(null);
  const [selectedProviderId, setSelectedProviderId] = useState('');
  const [query, setQuery] = useState('');
  const [loading, setLoading] = useState(true);
  /** 首次加载完成前才整页占位；后续刷新不得卸载当前页面（会丢失事务流程状态）。 */
  const [loaded, setLoaded] = useState(false);
  const [keyLoading, setKeyLoading] = useState(false);
  const [keyVersion, setKeyVersion] = useState(0);
  const [error, setError] = useState('');
  const [notice, setNotice] = useState('');
  const [providerEditor, setProviderEditor] = useState<Provider | 'new' | null>(null);
  const [modelEditor, setModelEditor] = useState<Model | 'new' | null>(null);
  const [keyEditor, setKeyEditor] = useState<Credential | 'new' | null>(null);
  const [switching, setSwitching] = useState(false);
  const selectedProvider = providers.find(p => p.id === selectedProviderId);
  const credentials = credentialsByProvider[selectedProviderId] ?? [];

  const refresh = useCallback(async () => {
    setLoading(true); setError('');
    try {
      const [providerResult, modelResult, gatewayResult] = await Promise.all([
        client.listProviders(), client.listModels(), client.gatewayStatus()]);
      setProviders(providerResult.items); setModels(modelResult); setGateway(gatewayResult);
      setSelectedProviderId(current => providerResult.items.some(p => p.id === current) ? current : providerResult.items[0]?.id ?? '');
    } catch (e) { setError(toCoreError(e).safeDetails.join('；') || '数据加载失败。'); }
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
    if (!providers.length) { setCredentialsByProvider({}); setKeyLoading(false); return; }
    setKeyLoading(true);
    Promise.all(providers.map(provider => client.listCredentials(provider.id)
      .then(items => [provider.id, items] as const)
      .catch(() => [provider.id, [] as Credential[]] as const)))
      .then(pairs => { if (current) setCredentialsByProvider(Object.fromEntries(pairs)); })
      .catch(e => { if (current) setError(toCoreError(e).safeDetails.join('；') || 'Key 列表加载失败。'); })
      .finally(() => { if (current) setKeyLoading(false); });
    return () => { current = false; };
  }, [client, providers, keyVersion]);

  function navigate(next: Page) { setPage(next); setQuery(''); setNotice(''); }
  function saved(kind: 'provider' | 'model' | 'key') {
    setProviderEditor(null); setModelEditor(null); setKeyEditor(null);
    setNotice(kind === 'key' ? 'Key 已安全保存。可以在列表中设为当前 Key。' : t('copy.draftSaved'));
    setKeyVersion(version => version + 1); void refresh();
  }
  async function selectKey(credential: Credential) {
    if (!selectedProvider) return;
    setSwitching(true); setError('');
    try {
      await client.selectCredential(selectedProvider.id, credential.id);
      setNotice(`已选择「${credential.label}」。模型应用后将使用此 Key。`);
      await refresh();
    } catch (e) { setError(toCoreError(e).safeDetails.join('；') || '切换 Key 失败。'); }
    finally { setSwitching(false); }
  }
  const pending = models.filter(m => m.inCatalog && m.hostState !== 'loaded');
  const search = query.trim().toLocaleLowerCase();
  const visibleProviders = providers.filter(p => `${p.name} ${p.endpoint}`.toLocaleLowerCase().includes(search));
  const visibleModels = models.filter(m => `${m.displayName} ${m.upstreamId} ${providers.find(p => p.id === m.providerId)?.name ?? ''}`.toLocaleLowerCase().includes(search));

  function modelTable(items: Model[]) {
    return items.length ? <div className={styles.tableWrap}><table><thead><tr><th scope="col">模型 / 上游 ID</th><th scope="col">供应商</th><th scope="col">上下文 / 最大输出</th><th scope="col">Codex 状态</th><th scope="col"><span className="visually-hidden">操作</span></th></tr></thead>
      <tbody>{items.map(model => <tr key={model.id}>
        <td><strong>{model.displayName}</strong><span className="text-mono text-muted break-anywhere">{model.upstreamId}</span></td>
        <td>{providers.find(p => p.id === model.providerId)?.name ?? '未知供应商'}</td>
        <td className="text-mono">{model.policy.contextLimit?.toLocaleString() ?? '未知'}<span className="text-muted">{model.policy.outputLimit?.toLocaleString() ?? '未指定'}</span></td>
        <td><span className={`badge ${model.hostState === 'pending_apply' ? 'warning' : ''}`}>{hostLabels[model.hostState]}</span></td>
        <td><button onClick={() => setModelEditor(model)} aria-label={`编辑 ${model.displayName}`}>编辑</button></td>
      </tr>)}</tbody></table></div> : <div className={styles.empty}><Boxes size={28} /><h3>{search ? '没有匹配的模型' : t('empty.noModelTitle')}</h3><p>{search ? '试试其他名称或模型 ID。' : '添加供应商后，手动填写模型 ID 和能力。'}</p>
        {!search && <button onClick={() => setModelEditor('new')} disabled={!providers.length}><Plus size={16} />添加模型</button>}</div>;
  }

  return <div className={styles.shell}>
    {/* 显式把焦点交给主内容：部分引擎不会为片段链接移动焦点。 */}
    <a className="skip-link" href="#main-content" onClick={() => document.getElementById('main-content')?.focus()}>跳到主要内容</a>
    <aside className={styles.sidebar}>
      <div className={styles.brand}><div className={styles.brandIcon}><ArrowLeftRight size={22} /></div><div><strong>GPTSwitch</strong><span>{t('app.subtitle')}</span></div></div>
      <nav aria-label="主导航">{navigation.map(({ id, icon: Icon }) => <button key={id} className={page === id ? styles.active : ''} aria-current={page === id ? 'page' : undefined} onClick={() => navigate(id)}><Icon size={18} />{t(`nav.${id}`)}</button>)}</nav>
      <div className={styles.sidebarBottom}><ShieldCheck size={18} /><div><strong>本地配置</strong><span>凭据使用系统安全存储</span></div></div>
      <div className={styles.version}>GPTSwitch <span>0.1.0 · 开发中</span></div>
    </aside>
    <div className={styles.workspace}>
      <div className={styles.topbar}><span>工作空间 <ChevronRight size={14} /> {t(`nav.${page}`)}</span>{gateway && !gateway.running ? <span className="badge warning">网关未启动</span> : pending.length ? <span className="badge warning">{pending.length} 个模型待应用</span> : gateway?.revisions.length ? <span className="badge">已应用到 Codex</span> : <span className="badge">尚未应用到 Codex</span>}</div>
      <main className={styles.main} id="main-content" tabIndex={-1}>
        <header className={styles.pageHeader}><div><h1 className="text-page-title">{t(`nav.${page}`)}</h1><p>{({ overview: '管理供应商与模型，让每一次切换都有清楚的状态。', providers: '服务地址、API Key 和模型，按供应商集中管理。', models: '定义模型身份、上下文、输入能力与思考选项。', codexConfig: '将保存的模型应用到 Codex 原生模型菜单。', diagnostics: '查看脱敏后的运行记录，定位失败原因。' })[page]}</p></div>
          <div className="actions"><button className="icon-button" aria-label="刷新数据" disabled={loading} onClick={() => void refresh()}><RefreshCw size={17} className={loading ? styles.spin : ''} /></button>
            {page !== 'codexConfig' && page !== 'diagnostics' && <button className="primary" disabled={loading || (page === 'models' && !providers.length)} onClick={() => page === 'models' ? setModelEditor('new') : setProviderEditor('new')}><Plus size={17} />{page === 'models' ? '添加模型' : '添加供应商'}</button>}</div></header>
        {error && <div className="error-message" role="alert">{error}</div>}
        {notice && <div className={styles.notice} role="status"><Check size={16} />{notice}</div>}
        {!loaded && !error ? <div className={styles.empty} role="status" aria-live="polite">正在读取本地配置…</div> : <>
          {page === 'overview' && <>
            <div className={styles.stats}>{[{ label: '供应商', value: providers.length, note: '独立地址与凭据' }, { label: '已保存模型', value: models.length, note: '支持自定义参数' }, { label: '待应用模型', value: pending.length, note: '保存后尚未进入原生菜单' }].map(stat => <div key={stat.label} className={styles.stat}><span>{stat.label}</span><strong>{stat.value.toString().padStart(2, '0')}</strong><small>{stat.note}</small></div>)}</div>
            <div className={styles.overviewGrid}>
              <section className={styles.card}><div className={styles.cardHeader}><h2>供应商</h2><button className="text-button" onClick={() => navigate('providers')}>管理全部 <ChevronRight size={15} /></button></div>
                {providers.length ? providers.slice(0, 4).map(provider => <button key={provider.id} className={styles.providerSummary} onClick={() => { setSelectedProviderId(provider.id); navigate('providers'); }}><div className={styles.monogram}>{provider.name.slice(0, 1)}</div><div><strong>{provider.name}</strong><span className="text-mono">{provider.endpoint}</span></div><ChevronRight size={16} /></button>) : <div className={styles.empty}><Server size={26} /><h3>从第一家供应商开始</h3><p>填写 API 地址，安全保存 Key，再添加你想使用的模型。</p><button onClick={() => setProviderEditor('new')}>添加供应商</button></div>}
              </section>
              <section className={styles.card}><div className={styles.cardHeader}><h2>接入步骤</h2><Settings2 size={18} /></div><ol className={styles.steps}><li><span>1</span><div><strong>添加供应商与 Key</strong><p>同一供应商可以保存多个 Key。</p></div></li><li><span>2</span><div><strong>配置模型能力</strong><p>填写精确模型 ID、上下文与思考档位。</p></div></li><li><span>3</span><div><strong>测试并应用到 Codex</strong><p>原生菜单加载后再确认实际请求路由。</p></div></li></ol><div className={styles.note}><CircleHelp size={17} /><p>配置保存、差异预览和应用/还原流程已接通。本机网关与连接检测尚未实现，因此应用后 Codex 还不能真正路由到第三方模型。</p></div></section>
            </div>
            <section className={styles.card}><div className={styles.cardHeader}><h2>待应用模型 <span className="badge">{pending.length}</span></h2><button className="text-button" onClick={() => navigate('models')}>查看全部 <ChevronRight size={15} /></button></div>{modelTable(pending)}</section>
          </>}
          {(page === 'providers' || page === 'models') && <div className={styles.toolbar}><div className={styles.search}><Search size={17} /><input aria-label={page === 'providers' ? '搜索供应商' : '搜索模型'} placeholder={page === 'providers' ? '搜索供应商名称或 API 地址' : '搜索名称、模型 ID 或供应商'} value={query} onChange={e => setQuery(e.target.value)} /></div><span>{page === 'providers' ? visibleProviders.length : visibleModels.length} 项</span></div>}
          {page === 'providers' && <div className={styles.providerLayout}><section className={styles.providerList} aria-label="供应商列表">
            {visibleProviders.map(provider => {
              const status = providerStatus(provider, credentialsByProvider[provider.id] ?? []);
              const selected = selectedProviderId === provider.id;
              return <div key={provider.id} className={`${styles.providerItem} ${selected ? styles.selected : ''}`}>
                <button className={styles.providerSelect} onClick={() => setSelectedProviderId(provider.id)} aria-current={selected ? 'true' : undefined}>
                  <div className={styles.monogram}>{provider.name.slice(0, 1)}</div>
                  <div className={styles.providerName}><strong>{provider.name}</strong><span>{provider.enabled ? '已启用' : '已停用'} · {models.filter(m => m.providerId === provider.id).length} 个模型</span></div>
                </button>
                <div className={styles.providerStatus}>
                  <span className={`${styles.statusDot} ${styles[status.tone]}`} aria-hidden="true" />
                  <span className={styles.statusLabel}>{status.label}</span>
                  <button onClick={() => { setSelectedProviderId(provider.id); setKeyEditor('new'); }}>{status.action}</button>
                </div>
              </div>;
            })}
            {!visibleProviders.length && <div className={styles.empty}><Server size={26} /><h3>{providers.length ? '没有匹配的供应商' : '还没有供应商'}</h3><p>点击右上角添加，开始配置。</p></div>}
          </section>{selectedProvider ? <section className={styles.card}>
            <div className={styles.cardHeader}><h2>{selectedProvider.name}</h2><button onClick={() => setProviderEditor(selectedProvider)}>编辑配置</button></div>
            <dl className={styles.details}><dt>API 地址</dt><dd className="text-mono break-anywhere">{selectedProvider.endpoint}</dd><dt>接口协议</dt><dd>{selectedProvider.protocol === 'responses' ? 'Responses' : 'Chat Completions · 待验证'}</dd><dt>认证方式</dt><dd>{selectedProvider.authKind === 'api_key' ? 'API Key' : '本机无认证'}</dd></dl>
            <div className={styles.cardHeader}><h3><KeyRound size={17} /> API Key</h3><button onClick={() => setKeyEditor('new')}><Plus size={16} />添加 Key</button></div>
            {keyLoading ? <p role="status">正在读取 Key 元数据…</p> : credentials.length ? <ul className={styles.keyList}>{credentials.map(credential => <li key={credential.id}><div><strong>{credential.label}</strong><span className="text-mono text-muted">{credential.maskedSuffix}</span></div><div className="actions"><button disabled={switching || selectedProvider.activeCredentialId === credential.id} onClick={() => void selectKey(credential)}>{selectedProvider.activeCredentialId === credential.id ? '当前 Key' : '设为当前'}</button><button onClick={() => setKeyEditor(credential)} aria-label={`替换 ${credential.label}`}>替换</button></div></li>)}</ul> : <div className={styles.empty}><KeyRound size={24} /><h3>还没有 API Key</h3><p>Key 按供应商独立保存，界面仅显示备注和掩码。</p></div>}
            <div className={styles.note}><ShieldCheck size={17} /><p>服务地址或 Key 已保存不代表连接成功。连接状态需要通过实际请求确认。</p></div>
            <ProbePanel client={client} provider={selectedProvider} credentialId={selectedProvider.activeCredentialId ?? null} />
          </section> : <section className={`${styles.card} ${styles.empty}`}><Settings2 size={28} /><p>选择供应商以管理它的配置和 Key。</p></section>}</div>}
          {page === 'models' && <section className={styles.card}>{modelTable(visibleModels)}</section>}
          {page === 'codexConfig' && <CodexConfigPage client={client} onApplied={() => void refresh()} />}
          {page === 'diagnostics' && <DiagnosticsPage client={client} />}
        </>}
      </main>
      <footer className={styles.statusbar}><span><span className={`${styles.dot} ${gateway?.running ? styles.online : styles.offline}`} />{gatewayText(gateway)}</span><span>{pending.length} 个模型待应用 <span className={styles.separator}>/</span> 配置保存在本机</span></footer>
    </div>
    {providerEditor && <ProviderForm client={client} provider={providerEditor === 'new' ? undefined : providerEditor} onSaved={() => saved('provider')} onClose={() => setProviderEditor(null)} />}
    {modelEditor && <ModelForm client={client} providers={providers} model={modelEditor === 'new' ? undefined : modelEditor} onSaved={() => saved('model')} onClose={() => setModelEditor(null)} />}
    {keyEditor && selectedProvider && <CredentialForm client={client} provider={selectedProvider} credential={keyEditor === 'new' ? undefined : keyEditor} onSaved={() => saved('key')} onClose={() => setKeyEditor(null)} />}
  </div>;
}
