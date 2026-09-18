import { useState, type FormEvent } from 'react';
import type { Provider } from '@/contracts/types';
import type { DesktopClient } from '@/desktop/client';
import { toCoreError } from '@/desktop/client';
import { Dialog } from '@/components/Dialog';

import { t } from '@/i18n';
export function ProviderForm({ client, provider, onSaved, onClose }: {
  client: DesktopClient; provider?: Provider; onSaved: () => void; onClose: () => void;
}) {
  const [busy, setBusy] = useState(false);
  const [dirty, setDirty] = useState(false);
  const [error, setError] = useState('');
  async function save(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const data = new FormData(event.currentTarget);
    setBusy(true); setError('');
    try {
      await client.saveProvider({ id: provider?.id, name: String(data.get('name')), endpoint: String(data.get('endpoint')),
        protocol: data.get('protocol') as Provider['protocol'], authKind: data.get('authKind') as Provider['authKind'],
        enabled: data.get('enabled') === 'on', notes: String(data.get('notes')) || null, presetId: provider?.presetId }, provider?.version ?? 0);
      onSaved();
    } catch (e) { setError(toCoreError(e).safeDetails.join(t('common.listSeparator')) || t('providers.saveFailed')); }
    finally { setBusy(false); }
  }
  return <Dialog title={provider ? t('providers.editProvider') : t('action.addProvider')} description={t('providers.saveHint')} dirty={dirty} busy={busy} onClose={onClose}>
    <form onSubmit={save} onChange={() => setDirty(true)}>
      <fieldset className="form-fields" disabled={busy}>
        <label>{t('providers.name')}<input name="name" required maxLength={64} defaultValue={provider?.name} placeholder={t('providers.namePlaceholder')} autoFocus /></label>
        <label>{t('providers.endpoint')}<input name="endpoint" type="url" required defaultValue={provider?.endpoint} placeholder="https://api.example.com/v1" spellCheck={false} /></label>
        <p className="field-hint">{t('providers.endpointPlaceholder')}</p>
        <div className="form-grid">
          <label>{t('providers.protocol')}<select name="protocol" defaultValue={provider?.protocol ?? 'responses'}><option value="responses">Responses</option><option value="chat_completions">{t('providers.protocolChatPending')}</option></select></label>
          <label>{t('providers.authKind')}<select name="authKind" defaultValue={provider?.authKind ?? 'api_key'}><option value="api_key">{t('auth.apiKey')}</option><option value="none">{t('providers.noAuth')}</option></select></label>
        </div>
        <label>{t('common.notes')} <span className="text-muted">{t('common.optional')}</span><textarea name="notes" maxLength={500} defaultValue={provider?.notes ?? ''} rows={2} /></label>
        <label className="check-label"><input name="enabled" type="checkbox" defaultChecked={provider?.enabled ?? true} />{t('providers.enable')}</label>
        {error && <div role="alert" className="error-message">{error}</div>}
      </fieldset>
      <footer className="form-footer"><span>{t('providers.keyStoredHint')}</span><button className="primary" disabled={busy}>{busy ? t('key.saving') : t('providers.saveProvider')}</button></footer>
    </form>
  </Dialog>;
}
