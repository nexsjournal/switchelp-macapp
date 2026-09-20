import { useMemo, useState } from 'react';
import { Search } from 'lucide-react';
import type { DiscoveredModel } from '@/desktop/client';
import { Dialog } from '@/components/Dialog';
import { toCoreError } from '@/desktop/client';
import { DISCOVERY_DEFAULT_LIMITS } from '@/features/models/policy';
import styles from './DiscoverModelsDialog.module.css';

import { t } from '@/i18n';

/**
 * 「获取可用模型」的结果弹窗。
 *
 * 上游返回的是一份**清单**，不是一个模型：只要求填一次地址和 Key，然后把想要的
 * 一次拿回来。所以这里是勾选 + 确认，而不是逐条「添加」再点开表单——
 * 加十个模型点十次弹窗，正是上一版最难受的地方。
 *
 * 两条必须说清的事实：
 * 1. 上游不返回上下文窗口，所以这次添加用的是默认长度（写在下方的提示里），
 *    添加完可以逐个打开编辑核对；
 * 2. 已经在库里的模型不会再加一次，它们显示成「已添加」且不可勾选。
 */
export function DiscoverModelsDialog({ models, busy, onAdd, onClose }: {
  models: DiscoveredModel[];
  busy: boolean;
  /** 保存选中的模型并在成功后关闭。失败时抛错，错误留在本弹窗里显示。 */
  onAdd: (selected: DiscoveredModel[]) => Promise<void>;
  onClose: () => void;
}) {
  const [query, setQuery] = useState('');
  const [error, setError] = useState('');
  /** 默认全选未添加的：上游返回清单就是想让人一次拿走，逐个勾是多余动作。 */
  const [selected, setSelected] = useState<string[]>(() => models.filter(model => !model.alreadySaved).map(model => model.upstreamId));

  const visible = useMemo(() => {
    const text = query.trim().toLocaleLowerCase();
    if (!text) return models;
    return models.filter(model => `${model.upstreamId} ${model.displayName}`.toLocaleLowerCase().includes(text));
  }, [models, query]);

  const selectable = visible.filter(model => !model.alreadySaved).map(model => model.upstreamId);
  const chosen = models.filter(model => selected.includes(model.upstreamId));

  const toggle = (id: string) => setSelected(current => current.includes(id) ? current.filter(item => item !== id) : [...current, id]);

  async function confirm() {
    setError('');
    try { await onAdd(chosen); }
    catch (thrown) { setError(toCoreError(thrown).safeDetails.join(t('common.listSeparator')) || t('common.failed')); }
  }

  return <Dialog title={t('providers.discoverTitle')} width="wide" busy={busy} onClose={onClose}
    description={t('providers.discoverBody', { count: models.length })}
    footer={<footer className="form-footer">
      <span>{t('providers.discoverDefaultHint', {
        context: DISCOVERY_DEFAULT_LIMITS.contextLimit.toLocaleString(),
        output: DISCOVERY_DEFAULT_LIMITS.outputLimit.toLocaleString() })}</span>
      <div className="actions">
        <button type="button" onClick={onClose} disabled={busy}>{t('action.cancel')}</button>
        <button type="button" className="primary" disabled={busy || chosen.length === 0} onClick={() => void confirm()}>
          {busy ? t('key.saving') : t('providers.discoverAdd', { count: chosen.length })}</button>
      </div></footer>}>
    <div className="form-fields">
      <div className={styles.toolbar}>
        <div className={styles.search}><Search size={16} />
          <input aria-label={t('providers.discoverSearch')} placeholder={t('models.searchPlaceholder')}
            value={query} onChange={event => setQuery(event.target.value)} />
        </div>
        <div className={styles.count}>
          {selectable.length > 1 && <button type="button" className="text-button"
            onClick={() => setSelected(current => current.length >= selectable.length ? [] : selectable)}>
            {selected.length >= selectable.length ? t('models.cancelSelection') : t('models.selectAll')}</button>}
          <span className="text-muted">{t('providers.discoverSelected', { count: chosen.length })}</span>
        </div>
      </div>

      {error && <div className="error-message" role="alert">{error}</div>}

      {visible.length === 0
        ? <p className="field-hint">{t('providers.discoverNoMatch')}</p>
        : <ul className={styles.list}>{visible.map(model => <li key={model.upstreamId}>
          <label className={styles.row}>
            <input type="checkbox" disabled={model.alreadySaved || busy}
              checked={model.alreadySaved || selected.includes(model.upstreamId)}
              onChange={() => toggle(model.upstreamId)} />
            <span className={styles.id}>{model.upstreamId}</span>
            <span className="text-muted break-anywhere">{model.displayName}</span>
            {model.alreadySaved && <span className="badge">{t('providers.added')}</span>}
          </label>
        </li>)}</ul>}
    </div>
  </Dialog>;
}
