import { useMemo, useRef, useState, type FormEvent } from 'react';
import { ChevronLeft, CircleHelp, PanelRightClose, PanelRightOpen } from 'lucide-react';
import type { Model, Provider } from '@/contracts/types';
import { type DesktopClient, isCoreError } from '@/desktop/client';
import { Dialog } from '@/components/Dialog';
import { defaultPolicy, inputLabels, parseTokens, policyFromForm } from './policy';
import styles from './ModelEditorPage.module.css';

/**
 * 模型编辑器（设计 P05）。
 *
 * 与对话框的取舍：设计的线框是**独立页面 + 右侧可折叠「生效预览」+ 固定页尾双按钮**。
 * 之所以值得做成页面：编辑一个模型要同时看「填了什么」和「这些值最终落在哪里」，
 * 右上角的生效预览就是回答后者——哪一项进 Codex 目录、哪一项只影响网关请求。
 *
 * 「保存并查看应用差异」= 保存后跳到 Codex 配置页生成差异；这里不复制差异逻辑。
 */

function SupportOptions() {
  return <><option value="unknown">未知</option><option value="supported">支持</option><option value="unsupported">不支持</option></>;
}

/** 生效位置：描述每个字段最终落到哪里，而不是重复一遍标签。 */
function effectRows(model: Model | undefined) {
  const policy = model?.policy;
  return [
    { label: '显示名称', effect: 'Codex 目录', note: '出现在 Codex 模型选择器里的名字' },
    { label: '上游模型 ID', effect: '网关请求', note: '请求上游时使用的精确 ID；改动会生成新的目录身份' },
    { label: '上下文窗口', effect: 'Codex 目录', note: '决定 Codex 何时压缩历史，不写全局覆盖' },
    { label: '最大输出', effect: '网关请求', note: `每次请求带上限；不保证模型输出恰好这么长${policy?.outputLimit ? `（当前 ${policy.outputLimit.toLocaleString()}）` : ''}` },
    { label: '压缩阈值', effect: '目录建议值', note: '留空时按上下文与输出自动建议' },
    { label: '输入能力', effect: '三层交集', note: '上游声明 ∩ 网关 ∩ 宿主，任何一层不支持就不会出现在原生能力里' },
    { label: '思考档位', effect: '目录 + 网关', note: '档位进目录；网关按已声明集合映射，未声明的不发送' },
  ];
}

export function ModelEditorPage({ client, providers, model, onSaved, onCancel, onViewDiff }: {
  client: DesktopClient;
  providers: Provider[];
  model?: Model;
  onSaved: () => Promise<void> | void;
  onCancel: () => void;
  onViewDiff?: () => void;
}) {
  const policy = model?.policy ?? defaultPolicy();
  const [busy, setBusy] = useState(false);
  const [dirty, setDirty] = useState(false);
  const [error, setError] = useState('');
  const [reasoning, setReasoning] = useState(policy.reasoning.support);
  const [control, setControl] = useState(policy.reasoning.control === 'none' ? 'effort' : policy.reasoning.control);
  const [showPreview, setShowPreview] = useState(true);
  const [discard, setDiscard] = useState(false);
  const [live, setLive] = useState({ context: policy.contextLimit?.toString() ?? '', output: policy.outputLimit?.toString() ?? '' });
  /** 两个提交按钮的差别只有“保存后是否跳去看差异”，用 ref 传意图最直接。 */
  const thenDiff = useRef(false);

  const rows = useMemo(() => effectRows(model), [model]);

  function leave() {
    if (dirty) setDiscard(true);
    else onCancel();
  }

  async function save(event: FormEvent<HTMLFormElement>, thenDiff: boolean) {
    event.preventDefault();
    const form = event.currentTarget;
    const data = new FormData(form);
    setBusy(true); setError('');
    try {
      await client.saveModel({
        id: model?.id,
        providerId: model?.providerId ?? String(data.get('providerId')),
        upstreamId: String(data.get('upstreamId')).trim(),
        displayName: String(data.get('displayName')).trim(),
        catalogAlias: model?.upstreamId === String(data.get('upstreamId')).trim() ? model.catalogAlias : '',
        policy: policyFromForm(data, policy),
        inCatalog: data.get('inCatalog') === 'on',
        displayNameOverridden: true,
      }, model?.version ?? 0);
      await onSaved();
      if (thenDiff) onViewDiff?.();
    } catch (thrown) {
      setError(isCoreError(thrown) ? thrown.safeDetails.join('；') || '保存失败，请刷新后重试。' : thrown instanceof Error ? thrown.message : '填写内容无效。');
    } finally { setBusy(false); }
  }

  return <div className={styles.page}>
    <header className={styles.header}>
      <div>
        <button className="text-button" onClick={leave}><ChevronLeft size={15} />模型</button>
        <h1 className="text-page-title">{model ? model.displayName : '新增模型'}</h1>
      </div>
      <div className="actions">
        <button onClick={() => setShowPreview(value => !value)} aria-expanded={showPreview}>
          {showPreview ? <PanelRightClose size={16} /> : <PanelRightOpen size={16} />}生效预览
        </button>
      </div>
    </header>

    {error && <div className="error-message" role="alert">{error}</div>}

    <div className={showPreview ? styles.layout : styles.single}>
      <form className={styles.form} onChange={() => setDirty(true)}
        onSubmit={event => { const diff = thenDiff.current; thenDiff.current = false; void save(event, diff); }}>
        <fieldset className="form-fields" disabled={busy}>
          <h3 className="form-section">基本信息</h3>
          <div className="form-grid">
            <label>供应商<select name="providerId" defaultValue={model?.providerId ?? providers[0]?.id} disabled={!!model} required>
              {providers.map(provider => <option key={provider.id} value={provider.id}>{provider.name}</option>)}
            </select></label>
            <label>显示名称<input name="displayName" defaultValue={model?.displayName} maxLength={96} required placeholder="在模型菜单中显示的名称" autoFocus /></label>
          </div>
          <label>上游模型 ID<input name="upstreamId" defaultValue={model?.upstreamId} required maxLength={256}
            placeholder="与供应商 API 的模型 ID 完全一致（大小写与斜杠都算数）" className="text-mono" spellCheck={false} /></label>

          <h3 className="form-section">长度限制 <span>上下包含历史、指令、工具信息与输出预留</span></h3>
          <div className="form-grid three">
            <label>上下文窗口（Token）<input name="contextLimit" defaultValue={policy.contextLimit ?? ''} placeholder="例如 128k 或 131072"
              onChange={event => setLive(current => ({ ...current, context: event.target.value }))} /></label>
            <label>最大输出（Token）<input name="outputLimit" defaultValue={policy.outputLimit ?? ''} placeholder="例如 8k 或 8192"
              onChange={event => setLive(current => ({ ...current, output: event.target.value }))} /></label>
            <label>压缩阈值<input name="compactLimit" defaultValue={policy.compactLimit ?? ''} placeholder="留空按建议值" /></label>
          </div>
          <p className="field-hint">最大输出是请求上限，不保证模型输出恰好这么长；它由本机网关按你填的值收口。</p>

          <h3 className="form-section">输入能力 <span>供应商声明</span></h3>
          <div className="capability-grid">{policy.inputs.map(input => <label key={input.kind}>
            {inputLabels[input.kind]}<select name={`input-${input.kind}`} defaultValue={input.upstream}><SupportOptions /></select>
          </label>)}</div>
          <p className="field-hint">只填服务商文档里明确支持的能力。宿主与网关两层由本工具推导；PDF 与视频当前链路不可原生发送，即使声明也不会出现在原生能力里。</p>

          <h3 className="form-section">思考模式</h3>
          <div className="form-grid">
            <label>是否支持<select name="reasoningSupport" value={reasoning} onChange={event => setReasoning(event.target.value as typeof reasoning)}>
              <SupportOptions /></select></label>
            {reasoning === 'supported' && <label>控制方式<select name="reasoningControl" value={control} onChange={event => setControl(event.target.value as typeof control)}>
              <option value="effort">思考档位</option><option value="toggle">开启 / 关闭</option><option value="budget">Token 预算</option>
            </select></label>}
          </div>
          {reasoning === 'supported' && control !== 'toggle' && <div className="form-grid">
            <label>支持的取值<input name="allowedValues" defaultValue={policy.reasoning.allowedValues.join(', ')}
              placeholder={control === 'effort' ? 'low, medium, high' : '填写服务商允许的值'} /></label>
            <label>默认取值<input name="defaultValue" defaultValue={policy.reasoning.defaultValue ?? ''} placeholder="留空表示不指定" /></label>
          </div>}
          {reasoning === 'supported' && control === 'budget' && <label>推理预算（Token）<input name="budgetTokens" defaultValue={policy.reasoning.budgetTokens ?? ''} placeholder="例如 4k" /></label>}
          <p className="field-hint">档位是可添加的受校验集合，不是固定下拉。未声明的档位不会被发送，只记为损失。</p>

          <h3 className="form-section">工具调用</h3>
          <div className="form-grid">
            <label>函数工具<select name="functionTools" defaultValue={policy.tools.functionTools}><SupportOptions /></select></label>
            <label>并行工具<select name="parallelTools" defaultValue={policy.tools.parallelTools}><SupportOptions /></select></label>
          </div>

          <label className="check-label"><input type="checkbox" name="inCatalog" defaultChecked={model?.inCatalog ?? true} />加入待应用的 Codex 模型目录</label>
          <p className="field-hint">不勾选也可以保存，只是不会出现在 Codex 模型菜单里。</p>

          <div className={styles.formFooter}>
            <span>{dirty ? '有未保存的修改' : '尚未修改'}</span>
            <div className="actions">
              <button type="button" onClick={leave} disabled={busy}>取消</button>
              <button type="submit" disabled={busy}>{busy ? '保存中…' : '保存草稿'}</button>
              <button type="submit" className="primary" disabled={busy} onClick={() => { thenDiff.current = true; }}>保存并查看应用差异</button>
            </div>
          </div>
        </fieldset>
      </form>

      {showPreview && <aside className={styles.preview} aria-label="生效预览">
        <h3>生效预览</h3>
        <dl>
          {rows.map(row => <div key={row.label} className={styles.effectRow}>
            <dt>{row.label}</dt>
            <dd><span className={styles.effect}>{row.effect}</span><small>{row.note}</small></dd>
          </div>)}
        </dl>
        <div className={styles.previewNote}>
          <CircleHelp size={15} />
          <p>
            当前填写：上下文 {live.context ? `${parseTokens(live.context)?.toLocaleString() ?? '格式待确认'}` : '未填写'} ·
            最大输出 {live.output ? `${parseTokens(live.output)?.toLocaleString() ?? '格式待确认'}` : '未填写'}
          </p>
        </div>
        <p className={styles.previewNote}>目录类改动需要 Codex 重新加载后才会出现在菜单里；网关类改动对后续请求生效。</p>
      </aside>}
    </div>

    {discard && <Dialog title="放弃修改" dirty={false} description="有尚未保存的修改，确定放弃吗？" onClose={() => setDiscard(false)}>
      <div className="form-fields"><div className="form-footer">
        <span>放弃后无法恢复。</span>
        <div className="actions">
          <button onClick={() => setDiscard(false)} autoFocus>继续编辑</button>
          <button className="danger" onClick={() => { setDiscard(false); onCancel(); }}>放弃修改</button>
        </div>
      </div></div>
    </Dialog>}
  </div>;
}
