import { useMemo, useState, type FormEvent } from 'react';
import { ChevronLeft, CircleHelp, PanelRightClose, PanelRightOpen } from 'lucide-react';
import type { Model, Provider } from '@/contracts/types';
import { type DesktopClient, isCoreError } from '@/desktop/client';
import { Dialog } from '@/components/Dialog';
import { defaultPolicy, inputLabel, parseTokens, policyFromForm } from './policy';
import styles from './ModelEditorPage.module.css';

import { t } from '@/i18n';
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
  return <><option value="unknown">{t('editor.unknown')}</option><option value="supported">{t('editor.supported')}</option><option value="unsupported">{t('compat.unsupported')}</option></>;
}

/** 生效位置：描述每个字段最终落到哪里，而不是重复一遍标签。 */
function effectRows(model: Model | undefined) {
  const policy = model?.policy;
  return [
    { label: t('editor.displayName'), effect: t('effect.toCatalog'), note: t('effect.displayNameNote') },
    { label: t('editor.upstreamId'), effect: t('effect.toGateway'), note: t('effect.upstreamIdNote') },
    { label: t('effect.context'), effect: t('effect.toCatalog'), note: t('effect.contextNote') },
    { label: t('effect.output'), effect: t('effect.toGateway'), note: policy?.outputLimit
      ? t('effect.outputNoteWithValue', { value: policy.outputLimit.toLocaleString() })
      : t('effect.outputNote') },
    { label: t('editor.compact'), effect: t('effect.toSuggested'), note: t('effect.compactNote') },
    { label: t('editor.inputs'), effect: t('effect.toIntersection'), note: t('effect.inputsNote') },
    { label: t('editor.reasoningLevels'), effect: t('effect.toCatalogAndGateway'), note: t('effect.reasoningNote') },
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

  const rows = useMemo(() => effectRows(model), [model]);

  function leave() {
    if (dirty) setDiscard(true);
    else onCancel();
  }

  async function save(event: FormEvent<HTMLFormElement>, viewDiff: boolean) {
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
      // 只在“保存并查看差异”那个按钮真正提交成功后才跳转。以前用 ref 在 onClick 里
      // 置位，表单校验没过时 ref 已经置上，之后点“保存草稿”也会被带去差异页。
      if (viewDiff) onViewDiff?.();
    } catch (thrown) {
      setError(isCoreError(thrown) ? thrown.safeDetails.join(t('common.listSeparator')) || t('providers.saveFailed') : thrown instanceof Error ? thrown.message : t('editor.invalid'));
    } finally { setBusy(false); }
  }

  return <div className={styles.page}>
    <header className={styles.header}>
      <div>
        <button className="text-button" onClick={leave}><ChevronLeft size={15} />{t('diag.model')}</button>
        <h1 className="text-page-title">{model ? model.displayName : t('editor.new')}</h1>
      </div>
      <div className="actions">
        <button onClick={() => setShowPreview(value => !value)} aria-expanded={showPreview}>
          {showPreview ? <PanelRightClose size={16} /> : <PanelRightOpen size={16} />}{t('editor.preview')}
        </button>
      </div>
    </header>

    {error && <div className="error-message" role="alert">{error}</div>}

    {/* 提交意图取自真正触发提交的那个按钮：onSubmit 只在表单校验通过后触发，
        所以标志位不会像过去那样残留下来污染下一次提交。 */}
    <div className={showPreview ? styles.layout : styles.single}>
      <form className={styles.form} onChange={() => setDirty(true)}
        onSubmit={event => {
          const submitter = (event.nativeEvent as SubmitEvent).submitter as HTMLButtonElement | null;
          void save(event, submitter?.value === 'true');
        }}>
        <fieldset className="form-fields" disabled={busy}>
          <h3 className="form-section">{t('editor.basics')}</h3>
          <div className="form-grid">
            <label>{t('editor.provider')}<select name="providerId" defaultValue={model?.providerId ?? providers[0]?.id} disabled={!!model} required>
              {providers.map(provider => <option key={provider.id} value={provider.id}>{provider.name}</option>)}
            </select></label>
            <label>{t('editor.displayName')}<input name="displayName" defaultValue={model?.displayName} maxLength={96} required placeholder={t('editor.displayNamePlaceholder')} autoFocus /></label>
          </div>
          <label>{t('editor.upstreamId')}<input name="upstreamId" defaultValue={model?.upstreamId} required maxLength={256}
            placeholder={t('editor.upstreamIdPlaceholder')} className="text-mono" spellCheck={false} /></label>

          <h3 className="form-section">{t('editor.limits')}<span>{t('editor.limitsHint')}</span></h3>
          <div className="form-grid three">
            <label>{t('editor.context')}<input name="contextLimit" defaultValue={policy.contextLimit ?? ''} placeholder={t('editor.contextPlaceholder')}
              onChange={event => setLive(current => ({ ...current, context: event.target.value }))} /></label>
            <label>{t('editor.output')}<input name="outputLimit" defaultValue={policy.outputLimit ?? ''} placeholder={t('editor.outputPlaceholder')}
              onChange={event => setLive(current => ({ ...current, output: event.target.value }))} /></label>
            <label>{t('editor.compact')}<input name="compactLimit" defaultValue={policy.compactLimit ?? ''} placeholder={t('editor.compactPlaceholder')} /></label>
          </div>
          <p className="field-hint">{t('editor.outputHint')}</p>

          <h3 className="form-section">{t('editor.inputs')}<span>{t('editor.declaredByProvider')}</span></h3>
          <div className="capability-grid">{policy.inputs.map(input => <label key={input.kind}>
            {inputLabel(input.kind)}<select name={`input-${input.kind}`} defaultValue={input.upstream}><SupportOptions /></select>
          </label>)}</div>
          <p className="field-hint">{t('editor.inputsHint')}</p>

          <h3 className="form-section">{t('editor.reasoning')}</h3>
          <div className="form-grid">
            <label>{t('editor.reasoningSupport')}<select name="reasoningSupport" value={reasoning} onChange={event => setReasoning(event.target.value as typeof reasoning)}>
              <SupportOptions /></select></label>
            {reasoning === 'supported' && <label>{t('editor.reasoningControl')}<select name="reasoningControl" value={control} onChange={event => setControl(event.target.value as typeof control)}>
              <option value="effort">{t('editor.reasoningLevels')}</option><option value="toggle">{t('editor.reasoningToggle')}</option><option value="budget">{t('editor.reasoningBudget')}</option>
            </select></label>}
          </div>
          {reasoning === 'supported' && control !== 'toggle' && <div className="form-grid">
            <label>{t('editor.allowedValues')}<input name="allowedValues" defaultValue={policy.reasoning.allowedValues.join(', ')}
              placeholder={control === 'effort' ? 'low, medium, high' : t('editor.allowedValuesPlaceholderOther')} /></label>
            <label>{t('editor.defaultValue')}<input name="defaultValue" defaultValue={policy.reasoning.defaultValue ?? ''} placeholder={t('editor.defaultValuePlaceholder')} /></label>
          </div>}
          {reasoning === 'supported' && control === 'budget' && <label>{t('editor.budgetTokens')}<input name="budgetTokens" defaultValue={policy.reasoning.budgetTokens ?? ''} placeholder={t('editor.budgetPlaceholder')} /></label>}
          <p className="field-hint">{t('editor.reasoningHint')}</p>

          <h3 className="form-section">{t('editor.tools')}</h3>
          <div className="form-grid">
            <label>{t('editor.functionTools')}<select name="functionTools" defaultValue={policy.tools.functionTools}><SupportOptions /></select></label>
            <label>{t('editor.parallelTools')}<select name="parallelTools" defaultValue={policy.tools.parallelTools}><SupportOptions /></select></label>
          </div>

          <label className="check-label"><input type="checkbox" name="inCatalog" defaultChecked={model?.inCatalog ?? true} />{t('editor.inCatalog')}</label>
          <p className="field-hint">{t('editor.inCatalogHint')}</p>

          <div className={styles.formFooter}>
            <span>{dirty ? t('editor.hasChanges') : t('editor.noChanges')}</span>
            <div className="actions">
              <button type="button" onClick={leave} disabled={busy}>{t('action.cancel')}</button>
              <button type="submit" disabled={busy}>{busy ? t('editor.saving') : t('action.saveDraft')}</button>
              <button type="submit" className="primary" disabled={busy} name="viewDiff" value="true">{t('action.saveAndViewDiff')}</button>
            </div>
          </div>
        </fieldset>
      </form>

      {showPreview && <aside className={styles.preview} aria-label={t('editor.preview')}>
        <h3>{t('editor.preview')}</h3>
        <dl>
          {rows.map(row => <div key={row.label} className={styles.effectRow}>
            <dt>{row.label}</dt>
            <dd><span className={styles.effect}>{row.effect}</span><small>{row.note}</small></dd>
          </div>)}
        </dl>
        <div className={styles.previewNote}>
          <CircleHelp size={15} />
          <p>
            {t('effect.currentValues', {
              context: live.context ? parseTokens(live.context)?.toLocaleString() ?? t('effect.unparsable') : t('effect.notFilled'),
              output: live.output ? parseTokens(live.output)?.toLocaleString() ?? t('effect.unparsable') : t('effect.notFilled'),
            })}
          </p>
        </div>
        <p className={styles.previewNote}>{t('effect.reloadNote')}</p>
      </aside>}
    </div>

    {discard && <Dialog title={t('editor.discardTitle')} dirty={false} description={t('editor.discardBody')} onClose={() => setDiscard(false)}>
      <div className="form-fields"><div className="form-footer">
        <span>{t('editor.discardIrreversible')}</span>
        <div className="actions">
          <button onClick={() => setDiscard(false)} autoFocus>{t('editor.keepEditing')}</button>
          <button className="danger" onClick={() => { setDiscard(false); onCancel(); }}>{t('editor.discardTitle')}</button>
        </div>
      </div></div>
    </Dialog>}
  </div>;
}
