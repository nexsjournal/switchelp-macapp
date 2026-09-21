import { useEffect, useRef, useState, type FormEvent } from 'react';
import { ChevronRight } from 'lucide-react';
import type { Model, Protocol } from '@/contracts/types';
import { type DesktopClient, isCoreError } from '@/desktop/client';
import { CheckCell, CheckCells } from '@/components/CheckCell';
import { Dialog } from '@/components/Dialog';
import { FieldHelp } from '@/components/FieldHelp';
import { Switch } from '@/components/Switch';
import { LevelChips } from './LevelChips';
import {
  DISCOVERY_DEFAULT_LIMITS, EDITABLE_INPUT_KINDS, capabilityState, defaultPolicy, inputBlocked, inputLabel,
  parseTokens, policyFromCapability, reasoningKeptKey, reasoningLevelPresets, type CapabilityState,
} from './policy';
import styles from './ModelFormDialog.module.css';

import { t } from '@/i18n';

/**
 * 「添加模型」弹窗（参考界面的高频路径）。
 *
 * 三件事必须一眼看到：上游模型 ID、上下文窗口、最大输出。**显示名不在这里**——
 * 上游模型 ID 就是它在 Codex 菜单里的名字，想另起名字去整页编辑器。
 *
 * 其余能力（输入类型、工具、思考档位）收进默认折叠的「高级配置」：它们决定这个模型
 * 在 Codex 里能用什么，但上游不返回这些值，第一次接入时不该拦着人先填空。
 *
 * 「智能配置」= 长度用推荐的默认值补齐（上游不返回窗口大小）。它不会替用户声明能力：
 * 能力项保持「未确认」，除非人自己点过。
 */
export function ModelFormDialog({ client, providerId, model, onSaved, onClose }: {
  client: DesktopClient;
  /** 供应商在弹窗上下文里是固定的：跨供应商的新建走模型目录页的整页编辑器。 */
  providerId: string;
  model?: Model;
  onSaved: (saved: Model) => Promise<void> | void;
  onClose: () => void;
}) {
  const policy = model?.policy ?? defaultPolicy();
  const [busy, setBusy] = useState(false);
  const [dirty, setDirty] = useState(false);
  const [error, setError] = useState('');
  /** 智能配置：长度留空时用推荐默认值补齐。 */
  const [smart, setSmart] = useState(true);
  const [upstreamId, setUpstreamId] = useState(model?.upstreamId ?? '');
  const [context, setContext] = useState(policy.contextLimit?.toString() ?? '');
  const [output, setOutput] = useState(policy.outputLimit?.toString() ?? '');
  const [ability, setAbility] = useState<CapabilityState>(() => capabilityState(policy));
  /** 档位 chip 只表达「档位式」：没动过就别改写已保存的声明（核心还支持开关式与预算式）。 */
  const [reasoningTouched, setReasoningTouched] = useState(false);
  /** 协议覆盖：默认跟随供应商。同一家上游可能一套模型走 Responses、另一套只有 chat/completions。 */
  const [protocolOverride, setProtocolOverride] = useState<Protocol | null>(model?.protocolOverride ?? null);
  const formRef = useRef<HTMLFormElement>(null);

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

  /** 长度留空时智能配置补什么值：写成占位符让人看得见，而不是保存时才偷偷填。 */
  const fallback = (value: string, limit: number) => smart && !value.trim() ? limit.toLocaleString() : undefined;

  function reset() {
    setSmart(true); setContext(policy.contextLimit?.toString() ?? ''); setOutput(policy.outputLimit?.toString() ?? '');
    setUpstreamId(model?.upstreamId ?? '');
    setAbility(capabilityState(policy));
    setReasoningTouched(false);
    setError(''); setDirty(false);
  }

  async function save(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    setBusy(true); setError('');
    try {
      const next: CapabilityState = { ...ability,
        contextLimit: parseTokens(context) ?? (smart ? DISCOVERY_DEFAULT_LIMITS.contextLimit : null),
        outputLimit: parseTokens(output) ?? (smart ? DISCOVERY_DEFAULT_LIMITS.outputLimit : null) };
      const id = upstreamId.trim();
      const saved = await client.saveModel({
        id: model?.id,
        providerId,
        upstreamId: id,
        // 显示名跟着上游 ID 走；上游发现结果带出的名字在批量添加时已经用过了。
        displayName: model && model.upstreamId === id ? model.displayName : id,
        catalogAlias: model?.upstreamId === id ? model.catalogAlias : '',
        policy: policyFromCapability(next, policy, reasoningTouched),
        inCatalog: model?.inCatalog ?? true,
        displayNameOverridden: model?.displayNameLayer.overridden ?? false,
        protocolOverride,
      }, model?.version ?? 0);
      setDirty(false);
      await onSaved(saved);
    } catch (thrown) {
      setError(thrown instanceof Error && !isCoreError(thrown) ? thrown.message
        : isCoreError(thrown) ? thrown.safeDetails.join(t('common.listSeparator')) || t('providers.saveFailed') : t('editor.invalid'));
    } finally { setBusy(false); }
  }

  const keptReasoning = reasoningKeptKey(policy);

  return <Dialog title={model ? t('editor.edit') : t('action.addModel')} width="normal"
    description={t('editor.dialogHint')} dirty={dirty} busy={busy} onClose={onClose}
    footer={<footer className="form-footer">
      <button type="button" className="text-button" onClick={reset} disabled={busy}>{t('editor.reset')}</button>
      <div className="actions">
        <button type="button" onClick={onClose} disabled={busy}>{t('action.cancel')}</button>
        <button className="primary" type="submit" form="model-form" disabled={busy}>{busy ? t('key.saving') : t('action.save')}</button>
      </div></footer>}>
    <form id="model-form" ref={formRef} onSubmit={save} onChange={() => setDirty(true)}>
      <fieldset className="form-fields" disabled={busy}>
        <div className={styles.smartRow}>
          <span className="field-label">{t('editor.smart')}<FieldHelp text={t('editor.smartHint')} /></span>
          <Switch checked={smart} onChange={value => { setSmart(value); setDirty(true); }} label={t('editor.smart')} />
        </div>
        {/* 开关换行的解释放在开关下面：它是这一段的行为说明，不是某个字段的标签。 */}
        <p className={styles.smartNote}>{smart ? t('editor.smartOn', { context: DISCOVERY_DEFAULT_LIMITS.contextLimit.toLocaleString(), output: DISCOVERY_DEFAULT_LIMITS.outputLimit.toLocaleString() }) : t('editor.smartOff')}</p>

        <label>{t('editor.modelId')}<input value={upstreamId} required maxLength={256} autoFocus={!model}
          onChange={event => { setUpstreamId(event.target.value); setDirty(true); }}
          placeholder={t('editor.upstreamIdPlaceholder')} spellCheck={false} /></label>

        <label><span className="field-label">{t('editor.contextShort')}<FieldHelp text={t('editor.contextHint')} /></span>
          <input value={context} onChange={event => { setContext(event.target.value); setDirty(true); }}
            placeholder={fallback(context, DISCOVERY_DEFAULT_LIMITS.contextLimit) ?? t('editor.contextPlaceholder')} /></label>

        <label><span className="field-label">{t('editor.outputShort')}<FieldHelp text={t('editor.outputHint')} /></span>
          <input value={output} onChange={event => { setOutput(event.target.value); setDirty(true); }}
            placeholder={fallback(output, DISCOVERY_DEFAULT_LIMITS.outputLimit) ?? t('editor.outputPlaceholder')} /></label>

        <label><span className="field-label">{t('editor.protocol')}<FieldHelp text={t('editor.protocolHint')} /></span>
          {/* 默认跟随供应商：绝大多数模型不需要单独设协议，把「跟随」放在第一项。 */}
          <select aria-label={t('editor.protocol')} value={protocolOverride ?? ''}
            onChange={event => { setProtocolOverride(event.target.value === '' ? null : event.target.value as Protocol); setDirty(true); }}>
            <option value="">{t('editor.protocolFollowProvider')}</option>
            <option value="responses">Responses</option>
            <option value="chat_completions">Chat Completions</option>
          </select></label>

        <details className={styles.advanced}>
          <summary className={styles.advancedSummary}>
            <ChevronRight size={14} className={styles.chevron} aria-hidden="true" />
            {t('editor.advanced')}
          </summary>
          <div className={styles.advancedBody}>
            <h4 className={styles.groupTitle}>{t('editor.inputsTitle')}<FieldHelp text={t('editor.inputsHint')} /></h4>
            <CheckCells>{EDITABLE_INPUT_KINDS.map(kind => <CheckCell key={kind}
              label={inputLabel(kind)}
              // 文本是链路底线：Codex 只会发它，所以永远勾着且不可取消。
              hint={kind === 'text' ? t('editor.textAlways') : inputBlocked(kind) ? t('editor.inputBlockedNote') : t('editor.inputsHint')}
              checked={ability.inputs[kind] === 'supported'}
              locked={kind === 'text'}
              disabled={kind === 'text' || inputBlocked(kind)}
              onChange={next => setAbility(current => ({ ...current, inputs: { ...current.inputs, [kind]: next ? 'supported' : 'unsupported' } }))} />)}
            </CheckCells>

            <h4 className={styles.groupTitle}>{t('editor.abilities')}<FieldHelp text={t('editor.abilitiesHint')} /></h4>
            <CheckCells>
              <CheckCell label={t('editor.functionTools')} hint={t('editor.abilitiesHint')}
                checked={ability.functionTools === 'supported'}
                onChange={next => setAbility(current => ({ ...current, functionTools: next ? 'supported' : 'unsupported' }))} />
              <CheckCell label={t('editor.parallelTools')} hint={t('editor.abilitiesHint')}
                checked={ability.parallelTools === 'supported'}
                onChange={next => setAbility(current => ({ ...current, parallelTools: next ? 'supported' : 'unsupported' }))} />
            </CheckCells>
            <p className="field-hint">{t('editor.abilityTriState')}</p>

            <h4 className={styles.groupTitle}>{t('editor.levelsTitle')}<FieldHelp text={t('editor.reasoningHint')} /></h4>
            <LevelChips levels={ability.levels} defaultLevel={ability.defaultLevel} presets={reasoningLevelPresets()} busy={busy}
              onChange={next => { setAbility(current => ({ ...current, ...next })); setReasoningTouched(true); }} />
            <p className="field-hint">{keptReasoning ? t(keptReasoning) : t('editor.reasoningLevelsHint')}</p>

            <h4 className={styles.groupTitle}>{t('editor.mapping')}<FieldHelp text={t('editor.mappingHint')} /></h4>
            {/* 只读：映射是网关里已版本化的适配器（reasoning.effort.v1），不是可以随手改的文本。
                做成可编辑的输入框会让人以为填进去的东西会生效。 */}
            <p className={styles.mapping}>{ability.levels.length ? t('editor.mappingValue') : t('editor.mappingEmpty')}</p>
          </div>
        </details>

        {error && <div role="alert" className="error-message">{error}</div>}
      </fieldset>
    </form>
  </Dialog>;
}
