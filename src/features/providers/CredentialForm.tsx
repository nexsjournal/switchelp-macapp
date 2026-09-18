import { useState, type FormEvent } from 'react';
import type { Credential, Provider } from '@/contracts/types';
import { type DesktopClient, toCoreError } from '@/desktop/client';
import { Dialog } from '@/components/Dialog';

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
    } catch (e) { setError(`${toCoreError(e).safeDetails.join('；') || '保存失败'}。重试前请重新填写 Key。`); }
    finally { setBusy(false); }
  }
  return <Dialog title={credential ? '替换 API Key' : '添加 API Key'} description={`${provider.name} · 只保存到本机系统凭据库，保存后仅显示掩码。`} onClose={onClose} dirty={dirty} busy={busy}>
    <form onSubmit={save} onChange={() => setDirty(true)}>
      <fieldset className="form-fields" disabled={busy}>
        <label>Key 备注<input name="label" required maxLength={64} defaultValue={credential?.label} readOnly={!!credential} placeholder="例如：日常使用" autoFocus /></label>
        <label>API Key<input name="secret" type="password" required maxLength={4096} autoComplete="new-password" spellCheck={false} placeholder="粘贴你的 API Key" /></label>
        <p className="field-hint">保存与设为当前 Key 分开操作。替换不会覆盖正在使用的旧安全条目。</p>
        {error && <div role="alert" className="error-message">{error}</div>}
      </fieldset>
      <footer className="form-footer"><span>不会修改 Codex 登录账号</span><button className="primary" disabled={busy}>{busy ? '正在保存…' : '安全保存'}</button></footer>
    </form>
  </Dialog>;
}
