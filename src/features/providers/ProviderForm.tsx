import { useState, type FormEvent } from 'react';
import type { Provider } from '@/contracts/types';
import type { DesktopClient } from '@/desktop/client';
import { toCoreError } from '@/desktop/client';
import { Dialog } from '@/components/Dialog';

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
    } catch (e) { setError(toCoreError(e).safeDetails.join('；') || '保存失败，请刷新后重试。'); }
    finally { setBusy(false); }
  }
  return <Dialog title={provider ? '编辑供应商' : '添加供应商'} description="先保存服务地址，再添加 API Key 和模型。" dirty={dirty} busy={busy} onClose={onClose}>
    <form onSubmit={save} onChange={() => setDirty(true)}>
      <fieldset className="form-fields" disabled={busy}>
        <label>供应商名称<input name="name" required maxLength={64} defaultValue={provider?.name} placeholder="例如：我的模型服务" autoFocus /></label>
        <label>API 地址<input name="endpoint" type="url" required defaultValue={provider?.endpoint} placeholder="https://api.example.com/v1" spellCheck={false} /></label>
        <p className="field-hint">填写 Base URL，无需添加 /responses 或 /chat/completions。远程地址必须使用 HTTPS。</p>
        <div className="form-grid">
          <label>接口协议<select name="protocol" defaultValue={provider?.protocol ?? 'responses'}><option value="responses">Responses</option><option value="chat_completions">Chat Completions（适配待验证）</option></select></label>
          <label>认证方式<select name="authKind" defaultValue={provider?.authKind ?? 'api_key'}><option value="api_key">API Key</option><option value="none">无认证 · 仅本机服务</option></select></label>
        </div>
        <label>备注 <span className="text-muted">选填</span><textarea name="notes" maxLength={500} defaultValue={provider?.notes ?? ''} rows={2} /></label>
        <label className="check-label"><input name="enabled" type="checkbox" defaultChecked={provider?.enabled ?? true} />启用此供应商</label>
        {error && <div role="alert" className="error-message">{error}</div>}
      </fieldset>
      <footer className="form-footer"><span>Key 保存在系统凭据库</span><button className="primary" disabled={busy}>{busy ? '正在保存…' : '保存供应商'}</button></footer>
    </form>
  </Dialog>;
}
