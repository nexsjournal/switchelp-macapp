import { useState, type FormEvent } from 'react';
import type { Credential, Provider } from '@/contracts/types';
import { type DesktopClient, toCoreError } from '@/desktop/client';
import { Dialog } from '@/components/Dialog';

import { t } from '@/i18n';
export function CredentialForm({ client, provider, credential, onSaved, onClose }: {
  client: DesktopClient; provider: Provider; credential?: Credential; onSaved: () => void; onClose: () => void;
}) {
  const [busy, setBusy] = useState(false);
  const [dirty, setDirty] = useState(false);
  const [error, setError] = useState('');
  async function save(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const form = event.currentTarget;
    const data = new FormData(form);
    const secret = String(data.get('secret'));
    const input = form.elements.namedItem('secret') as HTMLInputElement;
    input.value = ''; // 不放入 React 状态、缓存或 localStorage。
    setBusy(true); setError('');
    try {
      if (credential) await client.replaceCredential(credential.id, secret, credential.version);
      else await client.addCredential(provider.id, String(data.get('label')), secret);
      onSaved();
    } catch (e) { setError(toCoreError(e).safeDetails.join(t('common.listSeparator')) || t('providers.saveFailedWithRetry')); }
    finally { setBusy(false); }
  }
  return <Dialog title={credential ? t('key.replaceTitle') : t('key.addTitle')} description={t('key.storageNote', { provider: provider.name })} onClose={onClose} dirty={dirty} busy={busy}>
    <form onSubmit={save} onChange={() => setDirty(true)}>
      <fieldset className="form-fields" disabled={busy}>
        <label>{t('key.label')}<input name="label" required maxLength={64} defaultValue={credential?.label} readOnly={!!credential} placeholder={t('key.labelPlaceholder')} autoFocus /></label>
        <label>{t('auth.apiKey')}<input name="secret" type="password" required maxLength={4096} autoComplete="new-password" spellCheck={false} placeholder={t('key.secretPlaceholder')} /></label>
        <p className="field-hint">{t('key.replaceHint')}</p>
        {error && <div role="alert" className="error-message">{error}</div>}
      </fieldset>
      <footer className="form-footer"><span>{t('key.noAccountChange')}</span><button className="primary" disabled={busy}>{busy ? t('key.saving') : t('key.save')}</button></footer>
    </form>
  </Dialog>;
}
