import { useEffect, useRef, useState } from 'react';
import { Boxes, Eye, EyeOff, Info, Pencil, PlugZap, Plus, Trash2 } from 'lucide-react';
import type { Credential, Model, Provider } from '@/contracts/types';
import { type DesktopClient, type DiscoveredModel, isCoreError, toCoreError } from '@/desktop/client';
import { Dialog } from '@/components/Dialog';
import { RowMenu } from '@/components/RowMenu';
import { Switch } from '@/components/Switch';
import { showToast, dismissTone } from '@/components/Toast';
import { ModelFormDialog } from '@/features/models/ModelFormDialog';
import { compactTokens, discoveredModelPolicy, modelDraft } from '@/features/models/policy';
import { DiscoverModelsDialog } from './DiscoverModelsDialog';
import styles from './ProviderForm.module.css';

import { t } from '@/i18n';

/** 一次「可以拿去请求上游」的准备结果。 */
type Ready = { providerId: string; credentialId: string | null };

/** 模型列表里的一次确认：删除模型先移出目录，那一步不可撤销要说清楚。 */
type ModelConfirm = { model: Model };

function isLoopback(endpoint: string): boolean {
  try {
    const host = new URL(endpoint).hostname.toLocaleLowerCase();
    return host === 'localhost' || host === '::1' || host === '[::1]' || /^127\./.test(host);
  } catch { return false; }
}

/**
 * 供应商的唯一条目：录入、Key、模型、测试连接都在这里。
 *
 * 版面照参考界面收成一条直线：**名称 → 地址 → API 格式 → API Key → 模型列表**。
 * 名称是普通表单字段（以前把它塞进弹窗标题里，没人看得出那是个能点的输入框）。
 * 多出来的东西（备注、无认证、启用/停用、删除）都收进「更多」菜单。
 *
 * 两处容易出错的地方，这里都写成了不变量：
 *
 * - **版本号必须取最新的那一份**。核心按版本号拒绝覆盖别人的修改，而「把新 Key 设为当前 Key」
 *   这类动作本身就会把供应商的版本推高（保存供应商 v1 → 选当前 Key → v2）。如果还攥着创建时
 *   拿到的 v1 去写，就会被判成冲突，界面上一句「保存失败」什么也说明不了。所以这里从宿主给的
 *   活列表里取记录，并在真的撞上冲突时重读一次再写。
 * - **一次动作只推一条提示**，位置由全局 Toast 宿主决定（右下角悬浮），不再在弹窗底部摆横幅。
 */
export function ProviderForm({ client, provider, providers, models, onSaved, onKeysChanged, onChanged, onClose }: {
  client: DesktopClient;
  provider?: Provider;
  /** 宿主持有的活列表：版本号以此为准，弹窗里的快照会过期。 */
  providers: Provider[];
  /** 全部模型；这里只展示当前供应商的，过滤在组件内做。 */
  models: Model[];
  onSaved: (saved: Provider) => Promise<void> | void;
  onKeysChanged: () => void;
  /** 供应商本身发生变化（新建、删除）后让宿主重读列表。 */
  onChanged: () => Promise<void> | void;
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

  /** 本次弹窗里刚建好的供应商：兜底用，一旦宿主列表里有它就让位给更活的那份。 */
  const [created, setCreated] = useState<Provider | null>(null);
  const savedId = created?.id ?? provider?.id;
  const target = providers.find(item => item.id === savedId) ?? created ?? provider;
  const targetId = target?.id;

  const [discovered, setDiscovered] = useState<DiscoveredModel[] | null>(null);
  const [modelDialog, setModelDialog] = useState<{ model?: Model } | null>(null);
  const [modelConfirm, setModelConfirm] = useState<ModelConfirm | null>(null);
  const [deleting, setDeleting] = useState(false);
  const [testing, setTesting] = useState('');

  const [busy, setBusy] = useState('');
  const [dirty, setDirty] = useState(false);
  const nameInput = useRef<HTMLInputElement>(null);

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

  function fail(thrown: unknown, fallback: string) {
    showToast(toCoreError(thrown).safeDetails.join(t('common.listSeparator')) || fallback, 'danger');
  }

  /**
   * 写一次供应商（必要时连带写 Key）。
   *
   * `version` 必须是当前最新版本，由调用方给出——冲突重试正是靠它。
   */
  async function write(version: number, values: { name: string; endpoint: string; typed: string }): Promise<Ready> {
    const saved = await client.saveProvider({
      id: target?.id, name: values.name, endpoint: values.endpoint,
      protocol, authKind, enabled, notes: notes.trim() || null, presetId: target?.presetId,
    }, version);

    let credentialId = activeId;
    if (authKind === 'api_key' && values.typed) {
      if (activeId) {
        credentialId = (await client.replaceCredential(activeId, values.typed, activeKey?.version ?? 1)).id;
      } else {
        const added = await client.addCredential(saved.id, t('key.defaultLabel'), values.typed);
        await client.selectCredential(saved.id, added.id);
        credentialId = added.id;
      }
      setSecret('');
    }
    setCreated(saved);
    setActiveId(credentialId);
    await onSaved(saved);
    onKeysChanged();
    setKeys(await client.listCredentials(saved.id).catch(() => keys));
    return { providerId: saved.id, credentialId };
  }

  /**
   * 把供应商与 Key 落库，返回可以拿去请求上游的那一份。
   *
   * Key 分两种情形：已经有当前 Key 时输入新值＝替换（秘密值换了，编号不变）；
   * 还没有时输入新值＝新增并设为当前。空输入不改动已有 Key——
   * 界面上显示的是掩码，把掩码当原值提交是绝对不可以的。
   */
  async function persist(): Promise<Ready | null> {
    const values = { name: name.trim(), endpoint: endpoint.trim(), typed: secret.trim() };
    if (!values.name) {
      showToast(t('providers.nameRequired'), 'danger');
      nameInput.current?.focus();
      return null;
    }
    if (!values.endpoint) { showToast(t('providers.endpointRequired'), 'danger'); return null; }
    if (authKind === 'api_key' && !activeId && !values.typed) {
      showToast(t('providers.keySecretRequired'), 'danger');
      return null;
    }
    setBusy('save');
    // 上一次失败的原因已经不再适用（人改了输入又点了一次），别让它继续挂在屏幕上。
    dismissTone('danger');
    try {
      return await write(target?.version ?? 0, values);
    } catch (thrown) {
      // 版本冲突＝这个供应商在我们手上之后又被写过（最典型的是「设为当前 Key」那一步）。
      // 重读最新版本再写一次，比让用户自己去「刷新后重试」有用得多。
      if (isCoreError(thrown) && thrown.code === 'CONFLICT') {
        try {
          const fresh = (await client.listProviders()).items.find(item => item.id === savedId);
          if (fresh) {
            setCreated(fresh);
            return await write(fresh.version, values);
          }
        } catch { /* 重读或重写失败：往下走，把原始冲突照实说出去 */ }
      }
      fail(thrown, t('providers.saveFailed'));
      return null;
    } finally { setBusy(''); }
  }

  /** 「保存」按钮：只落库，不动模型；模型相关的动作各走各的（见下面几个）。 */
  async function save() {
    if (await persist()) showToast(t('providers.saved'));
  }

  /**
   * 模型相关的动作（获取、添加）需要一份已落库的供应商与当前 Key。
   *
   * 已经保存过、这次也没改任何字段时**不写库**：地址没变就没必要再写一遍，
   * 白写一次还会撞上版本冲突。改了字段或还没保存过时才走 `persist`。
   */
  async function ensureReady(): Promise<Ready | null> {
    if (target && !dirty) return { providerId: target.id, credentialId: activeId };
    return persist();
  }

  /** 获取可用模型：需要地址与当前 Key，所以先确保它已经落库。 */
  async function discover() {
    const ready = await ensureReady();
    if (!ready) return;
    if (!ready.credentialId) { showToast(t('providers.selectKeyFirstToFetch'), 'danger'); return; }
    setBusy('discover');
    try {
      const list = await client.discoverModels(ready.providerId, ready.credentialId);
      if (!list.length) { showToast(t('providers.noUpstreamModels'), 'danger'); return; }
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
    await onChanged();
    showToast(t('providers.discoverAdded', { count: selected.length }));
  }

  /** 当前 Key 换一个：秘密值不变，只改「新请求用哪一个」。 */
  async function switchKey(credentialId: string) {
    if (!targetId) return;
    try {
      await client.selectCredential(targetId, credentialId);
      setActiveId(credentialId);
      onKeysChanged();
    } catch (thrown) { fail(thrown, t('providers.switchKeyFailed')); }
  }

  /** 手工加模型：落到同一个模型表单，保存后回到这张表里。 */
  function addModel() {
    if (target) { setModelDialog({}); return; }
    // 还没保存的供应商先落库：模型必须挂在某个供应商下面才存得下。
    void ensureReady().then(ready => { if (ready) setModelDialog({}); });
  }

  /** 测试连接：只读探测（读模型列表，不发真实生成），不产生费用。 */
  async function testModel(model: Model) {
    if (!target || !activeId) { showToast(t('providers.selectKeyFirst'), 'danger'); return; }
    const label = `${target.name} / ${model.displayName}`;
    setTesting(model.id);
    try {
      const report = await client.startProbe({ providerId: target.id, modelId: model.id, credentialId: activeId }, { includeGenerate: false });
      const failed = report.stages.find(stage => stage.status === 'failed');
      if (failed) showToast(t('providers.testFailed', { label, reason: t(failed.messageKey) }), 'danger');
      else showToast(t('providers.testPassed', { label }));
    } catch (thrown) {
      showToast(t('providers.testFailed', { label, reason: toCoreError(thrown).safeDetails.join(t('common.listSeparator')) || t('common.failed') }), 'danger');
    } finally { setTesting(''); }
  }

  /** 纳入 / 移出 Codex 目录。它是这个模型会不会出现在 Codex 菜单里的开关。 */
  async function toggleCatalog(model: Model, next: boolean) {
    try {
      await client.saveModel(modelDraft(model, { inCatalog: next }), model.version);
      await onChanged();
      showToast(t(next ? 'providers.movedIn' : 'providers.movedOut', { name: model.displayName }));
    } catch (thrown) { fail(thrown, t('common.failed')); }
  }

  /** 删除模型。已纳入目录的必须先移出，所以这里连着做两步，并先把后果说清楚。 */
  async function deleteModel(model: Model) {
    setBusy('delete');
    try {
      let current = model;
      if (current.inCatalog) current = await client.saveModel(modelDraft(current, { inCatalog: false }), current.version);
      await client.deleteModel(current.id, current.version);
      setModelConfirm(null);
      await onChanged();
      showToast(t('models.deleted'));
    } catch (thrown) { fail(thrown, t('common.failed')); }
    finally { setBusy(''); }
  }

  /** 删除供应商：Key 与模型一起删除并撤销安全条目，所以先确认。 */
  async function deleteProvider() {
    if (!targetId) return;
    setBusy('delete-provider');
    try {
      await client.deleteProvider(targetId);
      setDeleting(false);
      await onChanged();
      showToast(t('providers.deleted'));
      onClose();
    } catch (thrown) { fail(thrown, t('providers.deleteFailed')); }
    finally { setBusy(''); }
  }

  const menuItems = [
    { key: 'notes', label: showNotes ? t('providers.hideNotes') : t('common.notes'), onSelect: () => setShowNotes(current => !current) },
    {
      key: 'auth', label: t('providers.noAuth'),
      hint: isLoopback(endpoint) ? undefined : t('providers.noAuthLoopbackOnly'),
      disabled: !isLoopback(endpoint),
      onSelect: () => { setAuthKind(current => current === 'api_key' ? 'none' : 'api_key'); setDirty(true); },
    },
    {
      // 停用与否以前是标题旁的一个开关：它跟「改名、备注」一样是供应商的属性，
      // 摆在标题栏会被当成「这个弹窗的开关」。收进菜单，行上的状态仍然照实显示。
      key: 'enabled', label: enabled ? t('providers.disableProvider') : t('providers.enableProvider'),
      onSelect: () => { setEnabled(current => !current); setDirty(true); },
    },
    ...(target ? [{ key: 'delete', label: t('providers.deleteProvider'), danger: true, onSelect: () => setDeleting(true) }] : []),
  ];

  return <>
    <Dialog title={target?.name || t('action.addProvider')} leadingIcon={<Boxes size={20} />}
      headerActions={<RowMenu label={t('providers.moreActions')} items={menuItems} />}
      dirty={dirty} busy={working} onClose={onClose}
      footer={<footer className={styles.footer}>
        <span>{t('providers.keyStoredHint')}</span>
        <div className="actions">
          <button type="button" onClick={onClose} disabled={working}>{t('action.cancel')}</button>
          <button className="primary" type="button" onClick={() => void save()} disabled={working}>{busy === 'save' ? t('key.saving') : t('action.save')}</button>
        </div>
      </footer>}>

      <div className={styles.form}>
        <label>{t('providers.name')}
          <input ref={nameInput} value={name} required maxLength={64} aria-label={t('providers.name')}
            placeholder={t('providers.namePlaceholder')} autoFocus={!target} spellCheck={false}
            onChange={event => { setName(event.target.value); setDirty(true); }} /></label>
        <p className="field-hint">{t('providers.nameHint')}</p>

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
                    title={t('models.deleteModel')} onClick={() => setModelConfirm({ model })}><Trash2 size={16} /></button>
                  <Switch size="small" checked={model.inCatalog} label={t('providers.catalogSwitch', { name: model.displayName })}
                    onChange={next => void toggleCatalog(model, next)} />
                </div>
              </li>;
            })}</ul>}
        </section>
      </div>
    </Dialog>

    {target && discovered && <DiscoverModelsDialog models={discovered} busy={working}
      onAdd={addDiscovered} onClose={() => setDiscovered(null)} />}

    {target && modelDialog && <ModelFormDialog client={client} providerId={target.id} model={modelDialog.model}
      onSaved={async () => { setModelDialog(null); await onChanged(); showToast(t('copy.draftSaved')); }}
      onClose={() => setModelDialog(null)} />}

    {modelConfirm && <Dialog title={t('models.deleteModel')} busy={working}
      description={t('providers.deleteModelBody', { name: modelConfirm.model.displayName, upstream: modelConfirm.model.upstreamId })}
      onClose={() => setModelConfirm(null)}>
      <div className="form-fields"><div className="form-footer">
        <span>{t('common.irreversible')}</span>
        <div className="actions">
          <button type="button" onClick={() => setModelConfirm(null)} disabled={working}>{t('action.cancel')}</button>
          <button type="button" className="danger" autoFocus disabled={working}
            onClick={() => void deleteModel(modelConfirm.model)}>{t('models.deleteModel')}</button>
        </div>
      </div></div>
    </Dialog>}

    {deleting && target && <Dialog title={t('providers.deleteProvider')} busy={working}
      description={t('providers.deleteProviderBody', {
        name: target.name, keys: keys.length, models: providerModels.length,
      })} onClose={() => setDeleting(false)}>
      <div className="form-fields"><div className="form-footer">
        <span>{t('common.irreversible')}</span>
        <div className="actions">
          <button type="button" onClick={() => setDeleting(false)} disabled={working}>{t('action.cancel')}</button>
          <button type="button" className="danger" autoFocus disabled={working}
            onClick={() => void deleteProvider()}>{t('providers.deleteProvider')}</button>
        </div>
      </div></div>
    </Dialog>}
  </>;
}
