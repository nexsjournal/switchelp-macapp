import { useEffect, useMemo, useState } from 'react';
import { Boxes, Filter, Plus, Search } from 'lucide-react';
import type { Model, Provider } from '@/contracts/types';
import { type DesktopClient, toCoreError } from '@/desktop/client';
import { Dialog } from '@/components/Dialog';
import { EmptyState } from '@/components/EmptyState';
import { showToast } from '@/components/Toast';
import { RowMenu } from '@/components/RowMenu';
import { hostStateKeys, hostStateVariant, modelDraft } from './policy';
import styles from './ModelsPage.module.css';

import { currentLocale, t } from '@/i18n';
/** 列表里的可用性筛选。未纳入目录的模型不算“可用”，但与“未测试”是两件事。 */
type Availability = 'all' | 'in_catalog' | 'not_in_catalog';

type SortKey = 'name' | 'provider' | 'context';

const availabilityKeys: Record<Availability, string> = {
  all: 'models.all',
  in_catalog: 'models.inCatalog',
  not_in_catalog: 'models.notInCatalog',
};

/**
 * 模型目录页（设计 P04）。
 *
 * 行操作按规范分成主操作、次操作与菜单：三个按钮并排会把操作列撑到比数据列还宽。
 * 「测试」只做只读探测，不会产生供应商费用。
 *
 * 编辑不再在本页内换页：打开编辑器这件事上交给宿主（App 统一托管编辑器状态），
 * 这样侧栏导航在编辑器打开期间才能一致地处理脏表单，而不是把填写内容静默丢掉。
 */
export function ModelsPage({ client, providers, models, onChanged, providerScope, embedded, onEditModel }: {
  client: DesktopClient; providers: Provider[]; models: Model[];
  onChanged: () => Promise<void> | void;
  /** 限定到某个供应商：供应商页把这一段嵌进它的详情里，不再提供跨供应商的筛选。 */
  providerScope?: string;
  /** 嵌进别的卡片时不再自带卡片外框，避免卡片套卡片。 */
  embedded?: boolean;
  /** 「编辑」与「添加模型」都上报给宿主打开整页编辑器。 */
  onEditModel: (target: Model | 'new') => void;
}) {
  const [query, setQuery] = useState('');
  /**
   * 嵌入供应商详情时，作用域**不能**复制进 state：`useState(providerScope ?? 'all')` 只在挂载时取值，
   * 而切换供应商不会重挂载本组件，于是表格会一直停在上一个供应商的模型上（标题已经换了、内容没换）。
   * 作用域是宿主给的事实，直接派生；只有非嵌入时那个跨供应商下拉才需要自己的 state。
   */
  const [pickedProvider, setPickedProvider] = useState('all');
  const providerFilter = providerScope ?? pickedProvider;
  const [availability, setAvailability] = useState<Availability>('all');
  const [sort, setSort] = useState<{ key: SortKey; desc: boolean }>({ key: 'name', desc: false });
  const [selected, setSelected] = useState<string[]>([]);
  const [busy, setBusy] = useState('');
  const [confirm, setConfirm] = useState<{ title: string; body: string; label: string; danger?: boolean; run: () => Promise<void> } | null>(null);
  const [probe, setProbe] = useState<{ model: Model; stages: { stageKey: string; status: string; messageKey: string; elapsedMs?: number | null }[] } | null>(null);

  // 换供应商就丢掉上一家的勾选：批量条会照旧报「已选 N 个」，但那些模型已经不在屏幕上，
  // 「批量删除」于是可能删掉用户看不见的行。
  useEffect(() => { setSelected([]); }, [providerScope]);

  const providerName = (id: string) => providers.find(provider => provider.id === id)?.name ?? t('common.unknownProvider');

  const visible = useMemo(() => {
    const text = query.trim().toLocaleLowerCase();
    const rows = models.filter(model => {
      if (providerFilter !== 'all' && model.providerId !== providerFilter) return false;
      if (availability !== 'all' && (availability === 'in_catalog') !== model.inCatalog) return false;
      if (!text) return true;
      return `${model.displayName} ${model.upstreamId} ${model.catalogAlias} ${providerName(model.providerId)}`
        .toLocaleLowerCase().includes(text);
    });
    const direction = sort.desc ? -1 : 1;
    return rows.sort((a, b) => {
      if (sort.key === 'provider') {
        return direction * providerName(a.providerId).localeCompare(providerName(b.providerId), currentLocale())
          || a.displayName.localeCompare(b.displayName, currentLocale());
      }
      if (sort.key === 'context') {
        return direction * ((a.policy.contextLimit ?? 0) - (b.policy.contextLimit ?? 0));
      }
      return direction * a.displayName.localeCompare(b.displayName, currentLocale());
    });
  }, [models, providers, query, providerFilter, availability, sort]);

  const selectedModels = models.filter(model => selected.includes(model.id));
  const selectableIds = visible.map(model => model.id);

  /**
   * 跑一次动作。失败走全局 Toast：它是**这次动作**的结果，跟页面状态（读不到数据）不是一回事，
   * 不该在工具栏下面占一条常驻的红条。
   */
  async function run(label: string, work: () => Promise<void>) {
    setBusy(label);
    try { await work(); }
    catch (thrown) { showToast(toCoreError(thrown).safeDetails.join(t('common.listSeparator')) || t('common.failed'), 'danger'); }
    finally { setBusy(''); }
  }

  const finish = async (message: string) => { showToast(message); setSelected([]); await onChanged(); };

  function askBulk(kind: 'leave' | 'delete') {
    const targets = kind === 'leave' ? selectedModels.filter(model => model.inCatalog) : selectedModels.filter(model => !model.inCatalog);
    const blocked = kind === 'delete' ? selectedModels.filter(model => model.inCatalog).length : 0;
    const scope = new Set(targets.map(model => providerName(model.providerId))).size;
    setConfirm({
      title: kind === 'leave' ? t('models.bulkLeaveTitle') : t('models.bulkDeleteTitle'),
      body: [
        t('models.bulkBody', {
          selected: selectedModels.length, providers: scope, targets: targets.length,
          extra: blocked > 0 ? t('models.bulkBlocked', { count: blocked }) : '',
        }),
        kind === 'leave' ? t('models.bulkLeaveNote') : t('models.bulkDeleteNote'),
        targets.length === 0 ? t('models.bulkNothing') : '',
      ].filter(Boolean).join(' '),
      label: kind === 'leave' ? t('models.moveOutShort') : t('action.delete'),
      danger: kind === 'delete',
      run: async () => {
        let done = 0;
        try {
          for (const model of targets) {
            if (kind === 'leave') await client.saveModel(modelDraft(model, { inCatalog: false }), model.version);
            else await client.deleteModel(model.id, model.version);
            done += 1;
          }
        } catch {
          // 中途失败：**不**把原始错误再往外冒一条。下面按完成数统一说明，
          // 两个提示说同一件事只会让人以为发生了两种错误。
        }
        // 失败也要刷新：前面已经成功的改动必须出现在列表里，否则界面与服务端不一致，
        // 用户会以为整批都没生效。
        setSelected([]);
        await onChanged();
        showToast(done === targets.length
          ? t(kind === 'leave' ? 'models.bulkLeaveDone' : 'models.bulkDeleteDone', { count: done })
          : t('models.bulkPartial', { done, total: targets.length }), done === targets.length ? 'success' : 'danger');
      },
    });
  }

  /** 只读探测：不发真实生成请求，也不产生费用。 */
  const probeModel = (model: Model) => run('probe', async () => {
    const credentials = await client.listCredentials(model.providerId);
    const active = credentials.find(credential => credential.status !== 'disabled' && credential.status !== 'missing');
    if (!active) {
      showToast(t('models.probeNoKey', { name: providerName(model.providerId) }), 'danger');
      return;
    }
    const report = await client.startProbe(
      { providerId: model.providerId, modelId: model.id, credentialId: active.id },
      { includeGenerate: false },
    );
    setProbe({ model, stages: report.stages });
  });

  const toggleAll = () => setSelected(current => current.length === selectableIds.length ? [] : selectableIds);
  const toggleOne = (id: string) => setSelected(current => current.includes(id) ? current.filter(item => item !== id) : [...current, id]);

  const sortHeader = (key: SortKey, label: string) => (
    <th scope="col" aria-sort={sort.key === key ? (sort.desc ? 'descending' : 'ascending') : 'none'}>
      <button type="button" className={styles.sortButton} onClick={() => setSort(current => ({ key, desc: current.key === key ? !current.desc : false }))}>
        {label}{sort.key === key ? (sort.desc ? ' ↓' : ' ↑') : ''}
      </button>
    </th>
  );

  const body = <>
    <div className={embedded ? styles.toolbarEmbedded : styles.toolbar}>
      <div className={styles.search}><Search size={18} />
        <input aria-label={t('models.searchAria')} placeholder={t('models.searchPlaceholder')} value={query} onChange={event => setQuery(event.target.value)} />
      </div>
      <div className={styles.filters}>
        <Filter size={14} aria-hidden="true" />
        {/* 嵌进供应商详情时已经限定了供应商，就不再多给一个筛选。 */}
        {!providerScope && <label>{t('editor.provider')}<select aria-label={t('models.providerFilter')} value={providerFilter} onChange={event => setPickedProvider(event.target.value)}>
          <option value="all">{t('logs.levelAll')}</option>
          {providers.map(provider => <option key={provider.id} value={provider.id}>{provider.name}</option>)}
        </select></label>}
        <label>{t('models.catalog')}<select aria-label={t('models.catalogFilter')} value={availability} onChange={event => setAvailability(event.target.value as Availability)}>
          {(Object.keys(availabilityKeys) as Availability[]).map(key => <option key={key} value={key}>{t(availabilityKeys[key])}</option>)}
        </select></label>
      </div>
      {!embedded && <button className="primary" onClick={() => onEditModel('new')} disabled={!providers.length}><Plus size={18} />{t('action.addModel')}</button>}
    </div>


    {selected.length > 0 && <div className={styles.bulkBar} role="region" aria-label={t('models.bulkRegion')}>
      <span>{t('models.selectedSummary', { count: selected.length, providers: new Set(selectedModels.map(m => providerName(m.providerId))).size })}</span>
      <span className="text-muted">{t('models.deletableHint', { count: selectedModels.filter(m => !m.inCatalog).length })}</span>
      <div className={styles.bulkActions}>
        <button onClick={() => askBulk('leave')} disabled={busy !== ''}>{t('models.bulkLeave')}</button>
        <button className="danger" onClick={() => askBulk('delete')} disabled={busy !== ''}>{t('models.bulkDelete')}</button>
        <button onClick={() => setSelected([])}>{t('models.cancelSelection')}</button>
      </div>
    </div>}

    {visible.length === 0
      ? <section className={styles.card}><EmptyState icon={Boxes}
          title={models.length ? t('models.noMatch') : t('empty.noModelTitle')}
          description={models.length ? t('models.noMatchBody') : t('models.emptyBody')}
          action={!models.length && <button className="primary" onClick={() => onEditModel('new')} disabled={!providers.length}><Plus size={16} />{t('action.addModel')}</button>} /></section>
      : <section className={embedded ? styles.plain : styles.card}>
        <div className={styles.tableWrap}>
          <table>
            <thead><tr>
              <th scope="col" className={styles.checkCell}>
                <span className={styles.checkHit}>
                  <input type="checkbox" aria-label={t('models.selectAll')}
                    checked={selected.length > 0 && selected.length === selectableIds.length}
                    onChange={toggleAll} />
                </span>
              </th>
              {sortHeader('name', t('models.columnModel'))}
              {!providerScope && sortHeader('provider', t('editor.provider'))}
              {sortHeader('context', t('models.columnLimits'))}
              <th scope="col">{t('models.columnState')}</th>
              <th scope="col"><span className="visually-hidden">{t('common.actions')}</span></th>
            </tr></thead>
            <tbody>{visible.map(model => <tr key={model.id} className={selected.includes(model.id) ? styles.selectedRow : undefined}>
              <td className={styles.checkCell}>
                <span className={styles.checkHit}>
                  <input type="checkbox" aria-label={t('models.select', { name: model.displayName })}
                    checked={selected.includes(model.id)} onChange={() => toggleOne(model.id)} />
                </span>
              </td>
              <td><strong>{model.displayName}</strong><span className={`${styles.line} text-mono break-anywhere`}>{model.upstreamId}</span></td>
              {!providerScope && <td>{providerName(model.providerId)}</td>}
              <td className="text-mono">{model.policy.contextLimit?.toLocaleString() ?? t('common.undeclared')}<span className={styles.line}>{model.policy.outputLimit?.toLocaleString() ?? t('common.undeclared')}</span></td>
              <td><span className={`badge ${hostStateVariant(model.hostState)}`}>{t(hostStateKeys[model.hostState])}</span></td>
              <td><div className={styles.rowActions}>
                <button onClick={() => onEditModel(model)} aria-label={t('models.editAria', { name: model.displayName })}>{t('common.edit')}</button>
                <button onClick={() => void probeModel(model)} disabled={busy === 'probe'} aria-label={t('models.testAria', { name: model.displayName })}>{t('common.test')}</button>
                <RowMenu label={t('models.moreActions', { name: model.displayName })} items={[
                  { key: 'copy', label: t('action.copyModelId'), onSelect: () => void navigator.clipboard?.writeText(model.upstreamId).then(() => showToast(t('models.copied', { id: model.upstreamId })), () => showToast(t('common.copyFailed'), 'danger')) },
                  {
                    key: 'catalog', label: model.inCatalog ? t('models.moveOut') : t('models.moveIn'),
                    onSelect: () => setConfirm({
                      title: model.inCatalog ? t('models.moveOut') : t('models.moveIn'),
                      body: t(model.inCatalog ? 'models.moveOutBody' : 'models.moveInBody', { name: model.displayName }),
                      label: model.inCatalog ? t('models.moveOutShort') : t('models.moveInShort'),
                      run: async () => { await client.saveModel(modelDraft(model, { inCatalog: !model.inCatalog }), model.version); await finish(t('models.catalogUpdated')); },
                    }),
                  },
                  {
                    key: 'delete', label: t('models.deleteModel'), danger: true, disabled: model.inCatalog,
                    hint: model.inCatalog ? t('models.moveOutFirst') : undefined,
                    onSelect: () => setConfirm({
                      title: t('models.deleteModel'),
                      body: t('models.deleteBody', { name: model.displayName, upstream: model.upstreamId }),
                      label: t('models.deleteModel'), danger: true,
                      run: async () => { await client.deleteModel(model.id, model.version); await finish(t('models.deleted')); },
                    }),
                  },
                ]} />
              </div></td>
            </tr>)}</tbody>
          </table>
        </div>
        <p className={styles.count}>{t('models.count', { visible: visible.length })}{visible.length !== models.length ? t('models.countOfTotal', { total: models.length }) : ''}</p>
      </section>}

    {probe && <Dialog width="normal" title={t('models.probeTitle', { name: probe.model.displayName })} busy={busy === 'probe'}
      description={t('models.probeBody')}
      onClose={() => setProbe(null)}
      footer={<footer className="form-footer">
        <span />
        <div className="actions"><button onClick={() => setProbe(null)} autoFocus>{t('common.close')}</button></div>
      </footer>}>
      <div className="form-fields">
        <ul className={styles.stages}>{probe.stages.map(stage => <li key={stage.stageKey} className={styles[stage.status] ?? ''}>
          <strong>{t(`stage.${stage.stageKey}`)}</strong>
          <span>{t(`probeState.${stage.status}`)}</span>
          <span className="text-muted">{t(stage.messageKey)}</span>
          {stage.elapsedMs != null && <span className="text-mono text-muted">{stage.elapsedMs} ms</span>}
        </li>)}</ul>
      </div>
    </Dialog>}

    {confirm && <Dialog width="narrow" title={confirm.title} description={confirm.body} onClose={() => setConfirm(null)} busy={busy !== ''} footer={<footer className="form-footer">
        <span>{t('common.irreversible')}</span>
        <div className="actions">
          <button onClick={() => setConfirm(null)} disabled={busy !== ''}>{t('action.cancel')}</button>
          <button className={confirm.danger ? 'danger' : 'primary'} autoFocus disabled={busy !== ''} onClick={async () => {
            const action = confirm; setConfirm(null); await run('bulk', action.run);
          }}>{confirm.label}</button>
        </div>
      </footer>}
      />}
  </>;

  // 内嵌时不再自带页面级间距；表格那张卡本身就够当外框了。
  return embedded ? body : <div className={styles.page}>{body}</div>;
}
