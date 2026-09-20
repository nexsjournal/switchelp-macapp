import { useEffect, useState } from 'react';
import { Boxes, Check, Eye, EyeOff, Info, Pencil, PlugZap, Plus, Trash2, X } from 'lucide-react';
import type { Credential, Model, Provider } from '@/contracts/types';
import { type DesktopClient, type DiscoveredModel, toCoreError } from '@/desktop/client';
import { Dialog } from '@/components/Dialog';
import { RowMenu } from '@/components/RowMenu';
import { Switch } from '@/components/Switch';
import { ModelFormDialog } from '@/features/models/ModelFormDialog';
import { compactTokens, discoveredModelPolicy, modelDraft } from '@/features/models/policy';
import { DiscoverModelsDialog } from './DiscoverModelsDialog';
import styles from './ProviderForm.module.css';

import { t } from '@/i18n';

/** 底部的结果提示条：一条连接结论，成功会自己收起，失败留着让人能看完。 */
type Banner = { tone: 'success' | 'danger'; text: string };

/** 模型列表里的一次确认：删除模型先移出目录，那一步不可撤销要说清楚。 */
type ModelConfirm = { model: Model };

function isLoopback(endpoint: string): boolean {
  try {
    const host = new URL(endpoint).hostname.toLocaleLowerCase();
    return host === 'localhost' || host === '::1' || host === '[::1]' || /^127\./.test(host);
  } catch { return false; }
}

/**
 * 供应商的唯一条目：录入、Key、模型、连接检查都在这里。
 *
 * 版面照参考界面收成一条直线：**地址 → API 格式 → API Key → 模型列表**。
 * 上一版把「认证方式 / 协议说明 / 备注 / 凭证池 / 连接检查」全铺在一屏里，
 * 于是接入一个供应商要在两组概念之间来回看。这里的原则是：
 *
 * - 高频三件套（地址、格式、Key）永远在首屏，Key 就是**当前**那一个；
 * - 多出来的东西（备注、无认证）收进标题右侧的「更多」，不占版面；
 * - 模型的加入有两条路：从上游一次拿回来（勾选确认），或者手工填一个；
 *   两条路都在模型列表这一段的表头，结果都落在同一张表里；
 * - 供应商没保存时点「获取可用模型」会先把它存下来：地址和 Key 是这次请求的
 *   前提，让人先点一次「保存」再点「获取」纯属白走一步。
 */
export function ProviderForm({ client, provider, models, onSaved, onKeysChanged, onModelsChanged, onClose }: {
  client: DesktopClient;
  provider?: Provider;
  /** 全部模型；这里只展示当前供应商的，过滤在组件内做。 */
  models: Model[];
  onSaved: (saved: Provider) => Promise<void> | void;
  onKeysChanged: () => void;
  onModelsChanged: () => Promise<void> | void;
  onClose: () => void;
}) {
  const [name, setName] = useState(provider?.name ?? '');
  const [endpoint, setEndpoint] = useState(provider?.endpoint ?? '');
  const [protocol, setProtocol] = useState<Provider['protocol']>(provider?.protocol ?? 'chat_completions');
  const [authKind, setAuthKind] = useState<Provider['authKind']>(provider?.authKind ?? 'api_key');
  const [enabled, setEnabled] = useState(provider?.enabled ?? true);
  const [notes, setNotes] = useState(provider?.notes ?? '');
  const [showNotes, setShowNotes] = useState(false);

  const [secret, setSecret] = useState('');
  const [secretVisible, setSecretVisible] = useState(false);
  const [keys, setKeys] = useState<Credential[]>([]);
  const [activeId, setActiveId] = useState<string | null>(provider?.activeCredentialId ?? null);

  /** 本次弹窗里刚建好的供应商：非空就说明已经能向上游取模型、能往里存模型了。 */
  const [created, setCreated] = useState<Provider | null>(null);
  const target = created ?? provider;
  const targetId = target?.id;

  const [discovered, setDiscovered] = useState<DiscoveredModel[] | null>(null);
  const [modelDialog, setModelDialog] = useState<{ model?: Model } | null>(null);
  const [confirm, setConfirm] = useState<ModelConfirm | null>(null);
  const [testing, setTesting] = useState('');
  const [banner, setBanner] = useState<Banner | null>(null);

  const [busy, setBusy] = useState('');
  const [dirty, setDirty] = useState(false);
  const [error, setError] = useState('');

  const activeKey = keys.find(credential => credential.id === activeId) ?? null;
  const providerModels = models
    .filter(model => model.providerId === targetId)
    .sort((a, b) => a.displayName.localeCompare(b.displayName));
  const working = busy !== '';

  useEffect(() => {
    if (!targetId) { setKeys([]); return; }
    let current = true;
    client.listCredentials(targetId).then(items => { if (current) setKeys(items); }).catch(() => { if (current) setKeys([]); });
    return () => { current = false; };
  }, [client, targetId]);

  // 成功的结论自己收起（规范：普通反馈 4~6 秒）；失败留着，因为下一步要看它决定怎么办。
  useEffect(() => {
    if (banner?.tone !== 'success') return;
    const timer = window.setTimeout(() => setBanner(null), 6000);
    return () => window.clearTimeout(timer);
  }, [banner]);

  function fail(thrown: unknown, fallback: string) {
    setError(toCoreError(thrown).safeDetails.join(t('common.listSeparator')) || fallback);
  }

  /**
   * 把供应商与 Key 落库，返回可以拿去请求上游的那一份。
   *
   * Key 分两种情形：已经有当前 Key 时输入新值＝替换（秘密值换了，编号不变）；
   * 还没有时输入新值＝新增并设为当前。空输入不改动已有 Key——
   * 界面上显示的是掩码，把掩码当原值提交是绝对不可以的。
   */
  async function persist(): Promise<{ providerId: string; credentialId: string | null } | null> {
    const address = endpoint.trim();
    if (!address) { setError(t('providers.endpointRequired')); return null; }
    const typed = secret.trim();
    if (authKind === 'api_key' && !activeId && !typed) { setError(t('providers.keySecretRequired')); return null; }
    setBusy('save'); setError('');
    try {
      const saved = await client.saveProvider({
        id: target?.id, name: name.trim() || t('action.addProvider'), endpoint: address,
        protocol, authKind, enabled, notes: notes.trim() || null, presetId: target?.presetId,
      }, target?.version ?? 0);

      let credentialId = activeId;
      if (authKind === 'api_key' && typed) {
        if (activeId) {
          credentialId = (await client.replaceCredential(activeId, typed, activeKey?.version ?? 1)).id;
        } else {
          const added = await client.addCredential(saved.id, t('key.defaultLabel'), typed);
          await client.selectCredential(saved.id, added.id);
          credentialId = added.id;
        }
        setSecret('');
      }
      setCreated(saved);
      setActiveId(credentialId);
      setDirty(false);
      await onSaved(saved);
      onKeysChanged();
      setKeys(await client.listCredentials(saved.id).catch(() => keys));
      return { providerId: saved.id, credentialId };
    } catch (thrown) { fail(thrown, t('providers.saveFailed')); return null; }
    finally { setBusy(''); }
  }

  /** 「保存」按钮：只落库，不动模型；模型相关的动作各走各的（见下面几个）。 */
  async function save() {
    const ready = await persist();
    if (ready) setBanner({ tone: 'success', text: t('providers.saved') });
  }

  /**
   * 模型相关的动作（获取、添加）需要一份已落库的供应商与当前 Key。
   *
   * 已经保存过、这次也没改任何字段时**不写库**：地址没变就没必要再写一遍，
   * 白写一次还会撞上版本冲突。改了字段或还没保存过时才走 `persist`。
   */
  async function ensureReady(): Promise<{ providerId: string; credentialId: string | null } | null> {
    if (target && !dirty) return { providerId: target.id, credentialId: activeId };
    return persist();
  }

  /** 获取可用模型：需要地址与当前 Key，所以先确保它已经落库。 */
  async function discover() {
    setBanner(null);
    const ready = await ensureReady();
    if (!ready) return;
    if (!ready.credentialId) { setError(t('providers.selectKeyFirstToFetch')); return; }
    setBusy('discover'); setError('');
    try {
      const list = await client.discoverModels(ready.providerId, ready.credentialId);
      if (!list.length) { setBanner({ tone: 'danger', text: t('providers.noUpstreamModels') }); return; }
      setDiscovered(list);
    } catch (thrown) { fail(thrown, t('providers.discoverFailed')); }
    finally { setBusy(''); }
  }

  /** 确认添加：上游不返回长度，统一用默认值写入，之后逐个核对（提示写在弹窗底栏）。 */
  async function addDiscovered(selected: DiscoveredModel[]) {
    if (!targetId) return;
    for (const model of selected) {
      await client.saveModel({
        providerId: targetId, upstreamId: model.upstreamId,
        displayName: model.displayName || model.upstreamId, catalogAlias: '',
        policy: discoveredModelPolicy(), inCatalog: true, displayNameOverridden: false,
      }, 0);
    }
    setDiscovered(null);
    await onModelsChanged();
    setBanner({ tone: 'success', text: t('providers.discoverAdded', { count: selected.length }) });
  }

  /** 手工加模型：落到同一个模型表单，保存后回到这张表里。 */
  function addModel() {
    if (target) { setModelDialog({}); return; }
    // 还没保存的供应商先落库：模型必须挂在某个供应商下面才存得下。
    void ensureReady().then(ready => { if (ready) setModelDialog({}); });
  }

  /** 测试连接：只读探测（读模型列表，不发真实生成），不产生费用。 */
  async function testModel(model: Model) {
    if (!target || !activeId) { setBanner({ tone: 'danger', text: t('providers.selectKeyFirst') }); return; }
    setTesting(model.id); setBanner(null); setError('');
    try {
      const report = await client.startProbe({ providerId: target.id, modelId: model.id, credentialId: activeId }, { includeGenerate: false });
      const failed = report.stages.find(stage => stage.status === 'failed');
      setBanner(failed
        ? { tone: 'danger', text: t('providers.testFailed', { label: `${target.name} / ${model.displayName}`, reason: t(failed.messageKey) }) }
        : { tone: 'success', text: t('providers.testPassed', { label: `${target.name} / ${model.displayName}` }) });
    } catch (thrown) {
      setBanner({ tone: 'danger', text: t('providers.testFailed', { label: `${target.name} / ${model.displayName}`, reason: toCoreError(thrown).safeDetails.join(t('common.listSeparator')) || t('common.failed') }) });
    } finally { setTesting(''); }
  }

  /** 纳入 / 移出 Codex 目录。它是这个模型会不会出现在 Codex 菜单里的开关。 */
  async function toggleCatalog(model: Model, next: boolean) {
    setError('');
    try {
      await client.saveModel(modelDraft(model, { inCatalog: next }), model.version);
      await onModelsChanged();
      setBanner({ tone: 'success', text: t(next ? 'providers.movedIn' : 'providers.movedOut', { name: model.displayName }) });
    } catch (thrown) { fail(thrown, t('common.failed')); }
  }

  /** 删除模型。已纳入目录的必须先移出，所以这里连着做两步，并先把后果说清楚。 */
  async function deleteModel(model: Model) {
    setBusy('delete'); setError('');
    try {
      let current = model;
      if (current.inCatalog) current = await client.saveModel(modelDraft(current, { inCatalog: false }), current.version);
      await client.deleteModel(current.id, current.version);
      setConfirm(null);
      await onModelsChanged();
      setBanner({ tone: 'success', text: t('models.deleted') });
    } catch (thrown) { fail(thrown, t('common.failed')); }
    finally { setBusy(''); }
  }

  /** 当前 Key 换一个：秘密值不变，只改「新请求用哪一个」。 */
  async function switchKey(credentialId: string) {
    if (!targetId) return;
    setError('');
    try {
      await client.selectCredential(targetId, credentialId);
      setActiveId(credentialId);
      onKeysChanged();
    } catch (thrown) { fail(thrown, t('providers.switchKeyFailed')); }
  }

  const menuItems = [
    { key: 'notes', label: showNotes ? t('providers.hideNotes') : t('common.notes'), onSelect: () => setShowNotes(current => !current) },
    {
      key: 'auth', label: t('providers.noAuth'),
      hint: isLoopback(endpoint) ? undefined : t('providers.noAuthLoopbackOnly'),
      disabled: !isLoopback(endpoint),
      onSelect: () => { setAuthKind(current => current === 'api_key' ? 'none' : 'api_key'); setDirty(true); },
    },
  ];

  return <>
    <Dialog
      leadingIcon={<Boxes size={20} />}
      title={<>
        {/* 标题里必须有文字：名称输入框的 value 不算文本内容，少了这一句弹窗就没有名字。
            这段文字与输入框的值会一起被念出来（「供应商配置 示例供应商 A」），
            所以它写成一段不跟名称重复的固定说明。 */}
        <span className="visually-hidden">{t('providers.dialogLabel')}</span>
        <input className={styles.titleInput} value={name} maxLength={64} aria-label={t('providers.name')}
          placeholder={t('action.addProvider')} spellCheck={false}
          onChange={event => { setName(event.target.value); setDirty(true); }} />
      </>}
      headerActions={<>
        <Switch checked={enabled} label={t('providers.enable')} onChange={next => { setEnabled(next); setDirty(true); }} />
        <RowMenu label={t('providers.moreActions')} items={menuItems} />
      </>}
      dirty={dirty} busy={working} onClose={onClose}
      banner={banner && <div className={banner.tone === 'success' ? `${styles.banner} ${styles.bannerSuccess}` : `${styles.banner} ${styles.bannerDanger}`} role="status">
        {banner.tone === 'success' ? <Check size={16} aria-hidden="true" /> : <Info size={16} aria-hidden="true" />}
        <span>{banner.text}</span>
        <button type="button" className="icon-button" aria-label={t('common.close')} onClick={() => setBanner(null)}><X size={16} /></button>
      </div>}
      footer={<footer className={styles.footer}>
        <span>{t('providers.keyStoredHint')}</span>
        <div className="actions">
          <button type="button" onClick={onClose} disabled={working}>{t('action.cancel')}</button>
          <button className="primary" type="button" onClick={() => void save()} disabled={working}>{busy === 'save' ? t('key.saving') : t('action.save')}</button>
        </div>
      </footer>}>

      <div className={styles.form}>
        <label>{t('providers.baseUrl')}
          <input type="url" required value={endpoint} spellCheck={false} placeholder="https://api.example.com/v1"
            onChange={event => { setEndpoint(event.target.value); setDirty(true); }} /></label>
        <p className="field-hint">{t('providers.openAiUrlHint')}</p>

        <label><span className="field-label">{t('providers.apiFormat')}</span>
          <select value={protocol} onChange={event => { setProtocol(event.target.value as Provider['protocol']); setDirty(true); }}>
            <option value="chat_completions">{t('providers.formatChat')}</option>
            <option value="responses">{t('providers.formatResponses')}</option>
          </select></label>
        <p className="field-hint">{t('providers.anthropicUnsupported')}</p>
        {/* Chat Completions 的适配器在核心层标着「未通过工具调用门禁」。这个事实必须说出来，
            否则选它的人会以为自己拿到的和 Responses 一样稳。 */}
        {protocol === 'chat_completions' && <p className={styles.experimental} role="note">{t('providers.chatAdapterExperimental')}</p>}

        {authKind === 'api_key' ? <>
          {/* SecretField：保存后只显示掩码。输入框留空＝不动已有 Key，
              直接写新值＝替换（编号不变，指向它的配置继续有效）。 */}
          <label><span className="field-label">{t('auth.apiKey')}</span>
            <span className={styles.secret}>
              <input type={secretVisible ? 'text' : 'password'} value={secret} maxLength={4096} spellCheck={false}
                autoComplete="new-password" aria-label={t('auth.apiKey')} className={styles.secretInput}
                placeholder={activeKey ? t('providers.keySaved', { suffix: activeKey.maskedSuffix }) : t('key.secretPlaceholder')}
                onChange={event => { setSecret(event.target.value); setDirty(true); }} />
              <button type="button" className="icon-button" aria-label={secretVisible ? t('key.hide') : t('key.reveal')}
                onClick={() => setSecretVisible(current => !current)}>{secretVisible ? <EyeOff size={16} /> : <Eye size={16} />}</button>
            </span></label>
          {keys.length > 1 && <label><span className="field-label">{t('providers.currentKey')}</span>
            <select value={activeId ?? ''} onChange={event => void switchKey(event.target.value)}>
              {keys.map(credential => <option key={credential.id} value={credential.id}>{credential.label} · {credential.maskedSuffix}</option>)}
            </select></label>}
          <p className="field-hint">{t('providers.apiKeyHint')}</p>
        </> : <p className={styles.authNote}>{t('providers.noAuthOn')}</p>}

        {showNotes && <label>{t('common.notes')}
          <textarea value={notes} maxLength={500} rows={2}
            onChange={event => { setNotes(event.target.value); setDirty(true); }} /></label>}

        <section className={styles.models} aria-label={t('providers.modelListTitle')}>
          <div className={styles.modelsHead}>
            <h3>{t('providers.modelListTitle')}</h3>
            <div className="actions">
              <button type="button" onClick={() => void discover()} disabled={working || authKind === 'none'}>
                {busy === 'discover' ? t('providers.fetching') : t('providers.fetchModels')}</button>
              <button type="button" className="primary" onClick={addModel} disabled={working}><Plus size={16} />{t('action.addModel')}</button>
            </div>
          </div>
          <p className="field-hint">{authKind === 'none' ? t('providers.noAuthNoDiscover') : t('providers.fetchModelsHint')}</p>

          {providerModels.length === 0
            ? <p className={styles.emptyModels}><Info size={16} aria-hidden="true" />{t('providers.noModelsYet')}</p>
            : <ul className={styles.modelList}>{providerModels.map(model => {
              const vision = model.policy.inputs.find(entry => entry.kind === 'image')?.upstream === 'supported';
              return <li key={model.id}>
                <span className={styles.modelName}>{model.displayName}</span>
                {model.policy.contextLimit !== null && <span className="badge"
                  title={t('providers.contextBadge', { value: model.policy.contextLimit.toLocaleString() })}>{compactTokens(model.policy.contextLimit)}</span>}
                {vision && <span className="badge">{t('providers.badgeVision')}</span>}
                <div className={styles.modelActions}>
                  <button type="button" className="icon-button" disabled={testing === model.id}
                    aria-label={t('providers.testModelAria', { name: model.displayName })} title={t('action.testConnection')}
                    onClick={() => void testModel(model)}><PlugZap size={16} /></button>
                  <button type="button" className="icon-button" aria-label={t('models.editAria', { name: model.displayName })}
                    title={t('common.edit')} onClick={() => setModelDialog({ model })}><Pencil size={16} /></button>
                  <button type="button" className="icon-button danger" aria-label={t('providers.deleteModelAria', { name: model.displayName })}
                    title={t('models.deleteModel')} onClick={() => setConfirm({ model })}><Trash2 size={16} /></button>
                  <Switch size="small" checked={model.inCatalog} label={t('providers.catalogSwitch', { name: model.displayName })}
                    onChange={next => void toggleCatalog(model, next)} />
                </div>
              </li>;
            })}</ul>}
        </section>

        {error && <div className="error-message" role="alert">{error}</div>}
      </div>
    </Dialog>

    {target && discovered && <DiscoverModelsDialog models={discovered} busy={working}
      onAdd={addDiscovered} onClose={() => setDiscovered(null)} />}

    {target && modelDialog && <ModelFormDialog client={client} providerId={target.id} model={modelDialog.model}
      onSaved={async () => { setModelDialog(null); await onModelsChanged(); setBanner({ tone: 'success', text: t('copy.draftSaved') }); }}
      onClose={() => setModelDialog(null)} />}

    {confirm && <Dialog title={t('models.deleteModel')} busy={working}
      description={t('providers.deleteModelBody', { name: confirm.model.displayName, upstream: confirm.model.upstreamId })}
      onClose={() => setConfirm(null)}>
      <div className="form-fields"><div className="form-footer">
        <span>{t('common.irreversible')}</span>
        <div className="actions">
          <button type="button" onClick={() => setConfirm(null)} disabled={working}>{t('action.cancel')}</button>
          <button type="button" className="danger" autoFocus disabled={working}
            onClick={() => void deleteModel(confirm.model)}>{t('models.deleteModel')}</button>
        </div>
      </div></div>
    </Dialog>}
  </>;
}
