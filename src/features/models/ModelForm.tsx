import { useState, type FormEvent } from 'react';
import type { Model, Provider } from '@/contracts/types';
import { type DesktopClient, isCoreError } from '@/desktop/client';
import { Dialog } from '@/components/Dialog';
import { defaultPolicy, inputLabels, policyFromForm } from './policy';

function SupportOptions() { return <><option value="unknown">未知</option><option value="supported">支持</option><option value="unsupported">不支持</option></>; }

export function ModelForm({ client, providers, model, onSaved, onClose }: {
  client: DesktopClient; providers: Provider[]; model?: Model; onSaved: () => void; onClose: () => void;
}) {
  const policy = model?.policy ?? defaultPolicy();
  const [busy, setBusy] = useState(false);
  const [dirty, setDirty] = useState(false);
  const [error, setError] = useState('');
  const [reasoning, setReasoning] = useState(policy.reasoning.support);
  const [control, setControl] = useState(policy.reasoning.control === 'none' ? 'effort' : policy.reasoning.control);
  async function save(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const data = new FormData(event.currentTarget);
    setBusy(true); setError('');
    try {
      await client.saveModel({ id: model?.id, providerId: model?.providerId ?? String(data.get('providerId')),
        upstreamId: String(data.get('upstreamId')).trim(), displayName: String(data.get('displayName')),
        catalogAlias: model?.upstreamId === String(data.get('upstreamId')).trim() ? model.catalogAlias : '',
        policy: policyFromForm(data, policy), inCatalog: data.get('inCatalog') === 'on', displayNameOverridden: true }, model?.version ?? 0);
      onSaved();
    } catch (e) { setError(isCoreError(e) ? e.safeDetails.join('；') || '保存失败，请刷新后重试。' : e instanceof Error ? e.message : '填写内容无效。'); }
    finally { setBusy(false); }
  }
  return <Dialog title={model ? '编辑模型' : '添加模型'} description="模型能力按服务商实际支持情况填写。保存后还需要应用到 Codex。" onClose={onClose} dirty={dirty} busy={busy}>
    <form onSubmit={save} onChange={() => setDirty(true)}>
      <fieldset className="form-fields" disabled={busy}>
        <div className="form-grid">
          <label>供应商<select name="providerId" defaultValue={model?.providerId ?? providers[0]?.id} disabled={!!model} required>{providers.map(p => <option key={p.id} value={p.id}>{p.name}</option>)}</select></label>
          <label>显示名称<input name="displayName" defaultValue={model?.displayName} maxLength={96} required placeholder="在模型菜单中显示的名称" autoFocus /></label>
        </div>
        <label>上游模型 ID<input name="upstreamId" defaultValue={model?.upstreamId} required maxLength={256} placeholder="与供应商 API 的模型 ID 完全一致" className="text-mono" spellCheck={false} /></label>
        <h3 className="form-section">上下文与输出</h3>
        <div className="form-grid three">
          <label>上下文窗口<input name="contextLimit" defaultValue={policy.contextLimit ?? ''} placeholder="例如 128k" /></label>
          <label>最大输出<input name="outputLimit" defaultValue={policy.outputLimit ?? ''} placeholder="例如 8k" /></label>
          <label>压缩阈值<input name="compactLimit" defaultValue={policy.compactLimit ?? ''} placeholder="选填" /></label>
        </div>
        <p className="field-hint">单位为 Token，32k = 32,000。未知请留空；应用前需要明确上下文窗口。</p>
        <h3 className="form-section">输入能力 <span>供应商声明</span></h3>
        <div className="capability-grid">{policy.inputs.map(input => <label key={input.kind}>{inputLabels[input.kind]}<select name={`input-${input.kind}`} defaultValue={input.upstream}><SupportOptions /></select></label>)}</div>
        <p className="field-hint">声明支持不等于 Codex 可以发送。PDF、视频等能力会保留记录，实际发送路径另行验证。</p>
        <h3 className="form-section">思考模式</h3>
        <div className="form-grid">
          <label>是否支持<select name="reasoningSupport" value={reasoning} onChange={e => setReasoning(e.target.value as typeof reasoning)}><SupportOptions /></select></label>
          {reasoning === 'supported' && <label>控制方式<select name="reasoningControl" value={control} onChange={e => setControl(e.target.value as typeof control)}><option value="effort">思考档位</option><option value="toggle">开启 / 关闭</option><option value="budget">Token 预算</option></select></label>}
        </div>
        {reasoning === 'supported' && <>
          <div className="form-grid">
            <label>支持的取值<input name="allowedValues" defaultValue={policy.reasoning.allowedValues.join(', ')} placeholder={control === 'effort' ? 'low, medium, high' : '填写服务商允许的值'} /></label>
            <label>默认取值<input name="defaultValue" defaultValue={policy.reasoning.defaultValue ?? ''} placeholder="留空表示不指定" /></label>
          </div>
          {control === 'budget' && <label>推理预算<input name="budgetTokens" defaultValue={policy.reasoning.budgetTokens ?? ''} placeholder="例如 4k" /></label>}
          {control !== 'effort' && <p className="field-hint">开关与预算可以保存声明，尚需对应供应商适配，不能保证出现在原生思考菜单中。</p>}
        </>}
        <h3 className="form-section">工具调用</h3>
        <div className="form-grid"><label>函数工具<select name="functionTools" defaultValue={policy.tools.functionTools}><SupportOptions /></select></label><label>并行工具<select name="parallelTools" defaultValue={policy.tools.parallelTools}><SupportOptions /></select></label></div>
        <label className="check-label"><input type="checkbox" name="inCatalog" defaultChecked={model?.inCatalog ?? true} />加入待应用的 Codex 模型目录</label>
        {error && <div role="alert" className="error-message">{error}</div>}
      </fieldset>
      <footer className="form-footer"><span>保存不会重启 Codex</span><button className="primary" disabled={busy}>{busy ? '正在保存…' : '保存模型'}</button></footer>
    </form>
  </Dialog>;
}
