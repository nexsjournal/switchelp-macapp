import { useEffect, useRef, useState, type FormEvent } from 'react';
import { ChevronLeft, ChevronRight } from 'lucide-react';
import type { Model, Protocol, Provider } from '@/contracts/types';
import { type DesktopClient, isCoreError } from '@/desktop/client';
import { CheckCell, CheckCells } from '@/components/CheckCell';
import { Dialog } from '@/components/Dialog';
import { FieldHelp } from '@/components/FieldHelp';
import { Switch } from '@/components/Switch';
import { LevelChips } from './LevelChips';
import {
  EDITABLE_INPUT_KINDS, capabilityState, defaultPolicy, inputBlocked, inputLabel, parseTokens,
  policyFromCapability, reasoningKeptKey, type CapabilityState,
} from './policy';
import styles from './ModelEditorPage.module.css';

import { t } from '@/i18n';

/**
 * 模型编辑器（设计 P05）·深度修改用。
 *
 * 与「添加模型」弹窗的关系：弹窗只问三件事（模型 ID、上下文、最大输出），其余收起来；
 * 这个页面把同一套控件铺开，额外给出整页才放得下的东西——显示名称、供应商、
 * 压缩阈值、纳入目录开关。**控件形状两处完全一致**（勾选单元格、档位 chip 都是共用组件），
 * 所以从弹窗进来的人不会看到第二套交互。
 *
 * 版面：页头 + 滚动区 + 钉在底部的操作条。滚动只发生在中间的字段区，
 * 取消/保存始终留在原地——以前底栏跟着内容一起滚，填到一半就得先滚到底才能保存。
 *
 * 这里没有「生效预览」：它把每个字段再说一遍，读的人还得先理解「Codex 目录 / 网关请求 /
 * 三层交集」这套词汇。这些信息改为挂在每个字段自己的「?」上——就在你要填的那个值旁边，
 * 一句话说清它落在哪里、会有什么后果。
 */
export function ModelEditorPage({ client, providers, model, onSaved, onCancel, onDirtyChange }: {
  client: DesktopClient;
  providers: Provider[];
  model?: Model;
  onSaved: () => Promise<void> | void;
  onCancel: () => void;
  /**
   * 脏状态上报给宿主：侧栏导航要在离开前确认，不能把未保存的填写静默丢掉。
   * 编辑器自己仍然负责弹「继续编辑 / 放弃修改」，两条路径互不接管。
   */
  onDirtyChange?: (dirty: boolean) => void;
}) {
  const policy = model?.policy ?? defaultPolicy();
  const [busy, setBusy] = useState(false);
  const [dirty, setDirty] = useState(false);
  const [error, setError] = useState('');
  const [inCatalog, setInCatalog] = useState(model?.inCatalog ?? true);
  const [ability, setAbility] = useState<CapabilityState>(() => capabilityState(policy));
  /** 思考那一节的档位 chip 只表达「档位式」；用户没动过就别去改写已保存的声明。 */
  const [reasoningTouched, setReasoningTouched] = useState(false);
  const [discard, setDiscard] = useState(false);
  const formRef = useRef<HTMLFormElement>(null);

  /** 所有脏状态变化都走这里：内部弹确认框，宿主据此拦截侧栏导航。 */
  function updateDirty(next: boolean) {
    setDirty(next);
    onDirtyChange?.(next);
  }

  useEffect(() => {
    function onKeydown(event: KeyboardEvent) {
      if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === 's') {
        event.preventDefault();
        if (!busy) formRef.current?.requestSubmit();
      }
    }
    window.addEventListener('keydown', onKeydown);
    return () => window.removeEventListener('keydown', onKeydown);
  }, [busy]);

  function leave() {
    if (dirty) setDiscard(true);
    else onCancel();
  }

  async function save(event: FormEvent<HTMLFormElement>) {
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
        policy: policyFromCapability({ ...ability,
          contextLimit: parseTokens(String(data.get('contextLimit') ?? '')),
          outputLimit: parseTokens(String(data.get('outputLimit') ?? '')),
        }, policy, reasoningTouched),
        inCatalog,
        displayNameOverridden: true,
        protocolOverride: (String(data.get('protocolOverride') ?? '') || null) as Protocol | null,
      }, model?.version ?? 0);
      updateDirty(false);
      await onSaved();
    } catch (thrown) {
      setError(isCoreError(thrown) ? thrown.safeDetails.join(t('common.listSeparator')) || t('providers.saveFailed')
        : thrown instanceof Error ? thrown.message : t('editor.invalid'));
    } finally { setBusy(false); }
  }

  const keptReasoning = reasoningKeptKey(policy);

  return <div className={styles.page}>
    <header className={styles.header}>
      <div>
        <button className="text-button" onClick={leave}><ChevronLeft size={14} />{t('diag.model')}</button>
        <h1 className="text-page-title">{model ? model.displayName : t('editor.new')}</h1>
      </div>
    </header>

    {/* 提交意图只来自这个表单自己的保存按钮：页面上不再有第二个「保存」，
        也就没有「上一次点了哪个按钮」这种需要记的状态。 */}
    <form ref={formRef} className={styles.form} onChange={() => updateDirty(true)} onSubmit={event => void save(event)}>
      <fieldset className={`form-fields ${styles.fields}`} disabled={busy}>
        <h3 className="form-section">{t('editor.basics')}</h3>
        <div className="form-grid">
          <label>{t('editor.provider')}
            <select name="providerId" defaultValue={model?.providerId ?? providers[0]?.id} disabled={!!model} required>
              {providers.map(provider => <option key={provider.id} value={provider.id}>{provider.name}</option>)}
            </select></label>
          <label><span className="field-label">{t('editor.displayName')}<FieldHelp text={t('editor.displayNameEffect')} /></span>
            <input name="displayName" defaultValue={model?.displayName} maxLength={96} required
              placeholder={t('editor.displayNamePlaceholder')} autoFocus /></label>
        </div>
        <label><span className="field-label">{t('editor.upstreamId')}<FieldHelp text={t('editor.upstreamIdEffect')} /></span>
          <input name="upstreamId" defaultValue={model?.upstreamId} required maxLength={256}
            placeholder={t('editor.upstreamIdPlaceholder')} spellCheck={false} /></label>

        <h3 className="form-section">{t('editor.limits')}</h3>
        <div className="form-grid">
          <label><span className="field-label">{t('editor.contextShort')}<FieldHelp text={t('editor.contextEffect')} /></span>
            <input name="contextLimit" defaultValue={policy.contextLimit ?? ''} placeholder={t('editor.contextPlaceholder')} /></label>
          <label><span className="field-label">{t('editor.outputShort')}<FieldHelp text={t('editor.outputEffect')} /></span>
            <input name="outputLimit" defaultValue={policy.outputLimit ?? ''} placeholder={t('editor.outputPlaceholder')} /></label>
        </div>
        <p className="field-hint">{t('editor.limitsHint')}</p>

        {/* 只有一个字段，不再另起一个小节标题：标题与字段名同为「接口协议」是重复的。 */}
        <label><span className="field-label">{t('editor.protocol')}<FieldHelp text={t('editor.protocolHint')} /></span>
          <select name="protocolOverride" defaultValue={model?.protocolOverride ?? ''}>
            <option value="">{t('editor.protocolFollowProvider')}</option>
            <option value="responses">Responses</option>
            <option value="chat_completions">Chat Completions</option>
          </select></label>

        <h3 className="form-section">{t('editor.inputsTitle')}</h3>
        <CheckCells>{EDITABLE_INPUT_KINDS.map(kind => <CheckCell key={kind}
          label={inputLabel(kind)}
          hint={kind === 'text' ? t('editor.textAlways') : inputBlocked(kind) ? t('editor.inputBlockedNote') : t('editor.inputsHint')}
          checked={ability.inputs[kind] === 'supported'}
          locked={kind === 'text'}
          disabled={kind === 'text' || inputBlocked(kind)}
          onChange={next => { setAbility(current => ({ ...current, inputs: { ...current.inputs, [kind]: next ? 'supported' : 'unsupported' } })); updateDirty(true); }} />)}
        </CheckCells>
        <p className="field-hint">{t('editor.inputsHint')}</p>

        <h3 className="form-section">{t('editor.abilities')}</h3>
        <CheckCells>
          <CheckCell label={t('editor.functionTools')} hint={t('editor.abilitiesHint')}
            checked={ability.functionTools === 'supported'}
            onChange={next => { setAbility(current => ({ ...current, functionTools: next ? 'supported' : 'unsupported' })); updateDirty(true); }} />
          <CheckCell label={t('editor.parallelTools')} hint={t('editor.abilitiesHint')}
            checked={ability.parallelTools === 'supported'}
            onChange={next => { setAbility(current => ({ ...current, parallelTools: next ? 'supported' : 'unsupported' })); updateDirty(true); }} />
        </CheckCells>
        <p className="field-hint">{t('editor.abilityTriState')}</p>

        <h3 className="form-section">{t('editor.levelsTitle')}</h3>
        <LevelChips levels={ability.levels} defaultLevel={ability.defaultLevel} busy={busy}
          onChange={next => { setAbility(current => ({ ...current, ...next })); setReasoningTouched(true); updateDirty(true); }} />
        <p className="field-hint">{keptReasoning ? t(keptReasoning) : t('editor.reasoningLevelsHint')}</p>

        <h3 className="form-section">{t('editor.catalog')}</h3>
        <div className={styles.switchRow}>
          <span className="field-label">{t('editor.inCatalog')}<FieldHelp text={t('editor.inCatalogEffect')} /></span>
          <Switch checked={inCatalog} label={t('editor.inCatalog')}
            onChange={next => { setInCatalog(next); updateDirty(true); }} />
        </div>
        <p className="field-hint">{t('editor.inCatalogHint')}</p>

        <details className={styles.advanced}>
          <summary className={styles.advancedSummary}>
            <ChevronRight size={14} className={styles.chevron} aria-hidden="true" />
            {t('editor.advanced')}
          </summary>
          <div className={styles.advancedBody}>
            <label><span className="field-label">{t('editor.compact')}<FieldHelp text={t('editor.compactEffect')} /></span>
              <input name="compactLimit" defaultValue={policy.compactLimit ?? ''} placeholder={t('editor.compactPlaceholder')} /></label>
          </div>
        </details>

        {error && <div role="alert" className="error-message">{error}</div>}
      </fieldset>

      <footer className={styles.formFooter}>
        <span>{dirty ? t('editor.hasChanges') : t('editor.noChanges')}　{t('editor.saveThenDiff')}</span>
        <div className="actions">
          <button type="button" onClick={leave} disabled={busy}>{t('action.cancel')}</button>
          <button type="submit" className="primary" disabled={busy}>{busy ? t('editor.saving') : t('action.save')}</button>
        </div>
      </footer>
    </form>

    {discard && <Dialog width="narrow" title={t('editor.discardTitle')} dirty={false} description={t('editor.discardBody')} onClose={() => setDiscard(false)} footer={<footer className="form-footer">
        <span>{t('editor.discardIrreversible')}</span>
        <div className="actions">
          <button onClick={() => setDiscard(false)} autoFocus>{t('editor.keepEditing')}</button>
          <button className="danger" onClick={() => { setDiscard(false); onCancel(); }}>{t('editor.discardTitle')}</button>
        </div>
      </footer>}
      />}
  </div>;
}
