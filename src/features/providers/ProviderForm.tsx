import { useState, type FormEvent } from 'react';
import type { Provider } from '@/contracts/types';
import type { DesktopClient } from '@/desktop/client';
import { toCoreError } from '@/desktop/client';
import { Dialog } from '@/components/Dialog';

import { t } from '@/i18n';

/** 保存供应商之后要不要顺手落一个 Key：Key 是它的必要条件，分两步走没有意义。 */
type KeyDraft = { secret: string; label: string };

/**
 * 添加/编辑供应商。
 *
 * 供应商、地址与 Key 在同一个弹窗里：用户要的是「把这家服务接进来」这一件事，
 * 拆成两个弹窗只会让人多点一次。Key 留空则只保存供应商（编辑时本来也不需要重填）。
 *
 * 地址分 OpenAI 与 Anthropic 两行：两种 API 家族的路径与消息格式不同。**当前只实现了
 * OpenAI 兼容协议**（`/chat/completions` 与 `/responses`），所以 Anthropic 那一行是
 * 禁用状态并写明原因，而不是收下来之后静默失败。
 */
export function ProviderForm({ client, provider, onSaved, onClose }: {
  client: DesktopClient; provider?: Provider; onSaved: () => void; onClose: () => void;
}) {
  const [busy, setBusy] = useState(false);
  const [dirty, setDirty] = useState(false);
  const [error, setError] = useState('');
  /** 只填了 Anthropic 地址时给出的明确说法，而不是让浏览器报一个泛泛的必填错误。 */
  const [anthropicOnly, setAnthropicOnly] = useState(false);

  function readKey(data: FormData): KeyDraft | null {
    const secret = String(data.get('secret') ?? '').trim();
    if (!secret) return null;
    return { secret, label: String(data.get('keyLabel') ?? '').trim() || t('key.defaultLabel') };
  }

  /** 保存供应商；如果这次填了 Key，再落库并设为当前 Key。 */
  async function persist(saved: Provider, key: KeyDraft | null) {
    if (!key) return;
    const credential = await client.addCredential(saved.id, key.label, key.secret);
    await client.selectCredential(saved.id, credential.id);
  }

  async function save(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const data = new FormData(event.currentTarget);
    const endpoint = String(data.get('endpoint') ?? '').trim();
    const anthropic = String(data.get('anthropicEndpoint') ?? '').trim();
    const key = readKey(data);
    // 两个地址至少要有一个；但 Anthropic 格式还没实现，所以只填它等于没法用。
    if (!endpoint) {
      setAnthropicOnly(Boolean(anthropic));
      setError(anthropic ? t('providers.onlyAnthropic') : t('providers.endpointRequired'));
      return;
    }
    setAnthropicOnly(false);
    setBusy(true); setError('');
    try {
      const saved = await client.saveProvider({
        id: provider?.id, name: String(data.get('name')), endpoint,
        protocol: data.get('protocol') as Provider['protocol'], authKind: data.get('authKind') as Provider['authKind'],
        enabled: data.get('enabled') === 'on', notes: String(data.get('notes')) || null, presetId: provider?.presetId,
      }, provider?.version ?? 0);
      await persist(saved, key);
      onSaved();
    } catch (e) { setError(toCoreError(e).safeDetails.join(t('common.listSeparator')) || t('providers.saveFailed')); }
    finally { setBusy(false); }
  }

  return <Dialog title={provider ? t('providers.editProvider') : t('action.addProvider')} description={t('providers.saveHint')} dirty={dirty} busy={busy} onClose={onClose}>
    <form onSubmit={save} onChange={() => setDirty(true)}>
      <fieldset className="form-fields" disabled={busy}>
        <label>{t('providers.name')}<input name="name" required maxLength={64} defaultValue={provider?.name} placeholder={t('providers.namePlaceholder')} autoFocus /></label>

        <label>{t('providers.openAiUrl')}<input name="endpoint" type="url" required={!dirty && !provider} defaultValue={provider?.endpoint} placeholder="https://api.example.com/v1" spellCheck={false} /></label>
        <p className="field-hint">{t('providers.openAiUrlHint')}</p>

        <label>{t('providers.anthropicUrl')}<input name="anthropicEndpoint" type="url" disabled placeholder="https://api.example.com/anthropic" spellCheck={false} /></label>
        <p className="field-hint">{t('providers.anthropicUnsupported')}</p>

        <div className="form-grid">
          <label>{t('providers.protocol')}<select name="protocol" defaultValue={provider?.protocol ?? 'responses'}><option value="responses">Responses</option><option value="chat_completions">{t('providers.protocolChatPending')}</option></select></label>
          <label>{t('providers.authKind')}<select name="authKind" defaultValue={provider?.authKind ?? 'api_key'}><option value="api_key">{t('auth.apiKey')}</option><option value="none">{t('providers.noAuth')}</option></select></label>
        </div>

        <div className="form-grid">
          <label>{t('providers.apiKeyOptional')}<input name="secret" type="password" maxLength={4096} autoComplete="new-password" spellCheck={false} placeholder={t('key.secretPlaceholder')} /></label>
          <label>{t('providers.keyLabelShort')}<input name="keyLabel" maxLength={64} placeholder={t('key.labelPlaceholder')} /></label>
        </div>
        <p className="field-hint">{t('providers.apiKeyHint')}</p>

        <label>{t('common.notes')} <span className="text-muted">{t('common.optional')}</span><textarea name="notes" maxLength={500} defaultValue={provider?.notes ?? ''} rows={2} /></label>
        <label className="check-label"><input name="enabled" type="checkbox" defaultChecked={provider?.enabled ?? true} />{t('providers.enable')}</label>
        {error && <div role="alert" className="error-message" data-anthropic-only={anthropicOnly} >{error}</div>}
      </fieldset>
      <footer className="form-footer"><span>{t('providers.keyStoredHint')}</span><button className="primary" disabled={busy}>{busy ? t('key.saving') : t('providers.saveProvider')}</button></footer>
    </form>
  </Dialog>;
}
