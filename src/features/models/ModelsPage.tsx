import { useMemo, useState } from 'react';
import { Boxes, Filter, Plus, Search } from 'lucide-react';
import type { Model, Provider } from '@/contracts/types';
import { type DesktopClient, toCoreError } from '@/desktop/client';
import { Dialog } from '@/components/Dialog';
import { RowMenu } from '@/components/RowMenu';
import { ModelEditorPage } from './ModelEditorPage';
import { modelDraft } from './policy';
import styles from './ModelsPage.module.css';

/** 列表里的可用性筛选。未纳入目录的模型不算“可用”，但与“未测试”是两件事。 */
type Availability = 'all' | 'in_catalog' | 'not_in_catalog';

type SortKey = 'name' | 'provider' | 'context';

const availabilityLabels: Record<Availability, string> = {
  all: '全部',
  in_catalog: '已纳入目录',
  not_in_catalog: '未纳入目录',
};

const hostLabels: Record<Model['hostState'], string> = {
  not_in_catalog: '未加入目录',
  pending_apply: '待应用',
  awaiting_reload: '等待重载',
  loaded: '已加载',
  load_unconfirmed: '加载未确认',
};

/**
 * 模型目录页（设计 P04）。
 *
 * 行操作按规范分成主操作、次操作与菜单：三个按钮并排会把操作列撑到比数据列还宽。
 * 「测试」只做只读探测，不会产生供应商费用。
 */
export function ModelsPage({ client, providers, models, onChanged, onViewDiff }: {
  client: DesktopClient; providers: Provider[]; models: Model[];
  onChanged: () => Promise<void> | void;
  /** 编辑器里的「保存并查看应用差异」需要跳到 Codex 配置页，这里只上报意图。 */
  onViewDiff?: () => void;
}) {
  const [query, setQuery] = useState('');
  const [providerFilter, setProviderFilter] = useState('all');
  const [availability, setAvailability] = useState<Availability>('all');
  const [sort, setSort] = useState<{ key: SortKey; desc: boolean }>({ key: 'name', desc: false });
  const [selected, setSelected] = useState<string[]>([]);
  const [editor, setEditor] = useState<Model | 'new' | null>(null);
  const [busy, setBusy] = useState('');
  const [error, setError] = useState('');
  const [notice, setNotice] = useState('');
  const [confirm, setConfirm] = useState<{ title: string; body: string; label: string; danger?: boolean; run: () => Promise<void> } | null>(null);
  const [probe, setProbe] = useState<{ model: Model; stages: { stageKey: string; status: string; messageKey: string; elapsedMs?: number | null }[] } | null>(null);

  const providerName = (id: string) => providers.find(provider => provider.id === id)?.name ?? '未知供应商';

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
        return direction * providerName(a.providerId).localeCompare(providerName(b.providerId), 'zh-Hans-CN')
          || a.displayName.localeCompare(b.displayName, 'zh-Hans-CN');
      }
      if (sort.key === 'context') {
        return direction * ((a.policy.contextLimit ?? 0) - (b.policy.contextLimit ?? 0));
      }
      return direction * a.displayName.localeCompare(b.displayName, 'zh-Hans-CN');
    });
  }, [models, providers, query, providerFilter, availability, sort]);

  const selectedModels = models.filter(model => selected.includes(model.id));
  const selectableIds = visible.map(model => model.id);

  async function run(label: string, work: () => Promise<void>) {
    setBusy(label); setError(''); setNotice('');
    try { await work(); }
    catch (thrown) { setError(toCoreError(thrown).safeDetails.join('；') || '操作失败。'); }
    finally { setBusy(''); }
  }

  const finish = async (message: string) => { setNotice(message); setSelected([]); await onChanged(); };

  function askBulk(kind: 'leave' | 'delete') {
    const targets = kind === 'leave' ? selectedModels.filter(model => model.inCatalog) : selectedModels.filter(model => !model.inCatalog);
    const blocked = kind === 'delete' ? selectedModels.filter(model => model.inCatalog).length : 0;
    const scope = new Set(targets.map(model => providerName(model.providerId))).size;
    setConfirm({
      title: kind === 'leave' ? '批量移出 Codex 目录' : '批量删除模型',
      body: [
        `已选 ${selectedModels.length} 个模型，涉及 ${scope} 家供应商，其中可执行 ${targets.length} 个。`,
        kind === 'leave' ? '移出后需要重新生成差异并应用，Codex 菜单才会更新。' : '删除不可撤销，本工具内的配置会立即消失。',
        blocked > 0 ? `另有 ${blocked} 个仍在目录中的模型不会被删除：请先移出目录。` : '',
        targets.length === 0 ? '当前没有可执行的目标。' : '',
      ].filter(Boolean).join(' '),
      label: kind === 'leave' ? '移出目录' : '删除',
      danger: kind === 'delete',
      run: async () => {
        for (const model of targets) {
          if (kind === 'leave') await client.saveModel(modelDraft(model, { inCatalog: false }), model.version);
          else await client.deleteModel(model.id, model.version);
        }
        await finish(kind === 'leave' ? `已移出 ${targets.length} 个模型。` : `已删除 ${targets.length} 个模型。`);
      },
    });
  }

  /** 只读探测：不发真实生成请求，也不产生费用。 */
  const probeModel = (model: Model) => run('probe', async () => {
    const credentials = await client.listCredentials(model.providerId);
    const active = credentials.find(credential => credential.status !== 'disabled' && credential.status !== 'missing');
    if (!active) {
      setError(`「${providerName(model.providerId)}」还没有可用的 Key，无法测试。`);
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

  if (editor) {
    return <ModelEditorPage client={client} providers={providers} model={editor === 'new' ? undefined : editor}
      onCancel={() => { setEditor(null); setError(''); }}
      onViewDiff={onViewDiff}
      onSaved={async () => { setEditor(null); await finish('已保存模型草稿。'); }} />;
  }

  return <div className={styles.page}>
    <div className={styles.toolbar}>
      <div className={styles.search}><Search size={17} />
        <input aria-label="搜索模型" placeholder="搜索名称、模型 ID 或供应商" value={query} onChange={event => setQuery(event.target.value)} />
      </div>
      <div className={styles.filters}>
        <Filter size={15} aria-hidden="true" />
        <label>供应商<select aria-label="供应商筛选" value={providerFilter} onChange={event => setProviderFilter(event.target.value)}>
          <option value="all">全部</option>
          {providers.map(provider => <option key={provider.id} value={provider.id}>{provider.name}</option>)}
        </select></label>
        <label>目录<select aria-label="目录筛选" value={availability} onChange={event => setAvailability(event.target.value as Availability)}>
          {(Object.keys(availabilityLabels) as Availability[]).map(key => <option key={key} value={key}>{availabilityLabels[key]}</option>)}
        </select></label>
      </div>
      <button className="primary" onClick={() => setEditor('new')} disabled={!providers.length}><Plus size={17} />添加模型</button>
    </div>

    {error && <div className="error-message" role="alert">{error}</div>}
    {notice && <div className={styles.notice} role="status">{notice}</div>}

    {selected.length > 0 && <div className={styles.bulkBar} role="region" aria-label="批量操作">
      <span>已选 <strong>{selected.length}</strong> 个 · 跨 <strong>{new Set(selectedModels.map(m => providerName(m.providerId))).size}</strong> 家供应商</span>
      <span className="text-muted">其中 {selectedModels.filter(m => !m.inCatalog).length} 个可直接删除</span>
      <div className={styles.bulkActions}>
        <button onClick={() => askBulk('leave')} disabled={busy !== ''}>批量移出目录</button>
        <button className="danger" onClick={() => askBulk('delete')} disabled={busy !== ''}>批量删除</button>
        <button onClick={() => setSelected([])}>取消选择</button>
      </div>
    </div>}

    {visible.length === 0
      ? <section className={styles.empty}>
        <Boxes size={28} />
        <h3>{models.length ? '没有匹配的模型' : '还没有模型'}</h3>
        <p>{models.length ? '换个关键词，或把筛选调回“全部”。' : '添加供应商后，手动填写模型 ID 和能力；也可以从上游获取模型列表。'}</p>
        {!models.length && <button onClick={() => setEditor('new')} disabled={!providers.length}><Plus size={16} />添加模型</button>}
      </section>
      : <section className={styles.card}>
        <div className={styles.tableWrap}>
          <table>
            <thead><tr>
              <th scope="col" className={styles.checkCell}>
                <input type="checkbox" aria-label="全选模型"
                  checked={selected.length > 0 && selected.length === selectableIds.length}
                  onChange={toggleAll} />
              </th>
              {sortHeader('name', '模型 / 上游 ID')}
              {sortHeader('provider', '供应商')}
              {sortHeader('context', '上下文 / 最大输出')}
              <th scope="col">Codex 状态</th>
              <th scope="col"><span className="visually-hidden">操作</span></th>
            </tr></thead>
            <tbody>{visible.map(model => <tr key={model.id} className={selected.includes(model.id) ? styles.selectedRow : undefined}>
              <td className={styles.checkCell}>
                <input type="checkbox" aria-label={`选择 ${model.displayName}`}
                  checked={selected.includes(model.id)} onChange={() => toggleOne(model.id)} />
              </td>
              <td><strong>{model.displayName}</strong><span className="text-mono text-muted break-anywhere">{model.upstreamId}</span></td>
              <td>{providerName(model.providerId)}</td>
              <td className="text-mono">{model.policy.contextLimit?.toLocaleString() ?? '未声明'}<span className="text-muted">{model.policy.outputLimit?.toLocaleString() ?? '未声明'}</span></td>
              <td><span className={`badge ${model.hostState === 'pending_apply' ? 'warning' : ''}`}>{hostLabels[model.hostState]}</span></td>
              <td><div className={styles.rowActions}>
                <button onClick={() => setEditor(model)} aria-label={`编辑 ${model.displayName}`}>编辑</button>
                <button onClick={() => void probeModel(model)} disabled={busy === 'probe'} aria-label={`测试 ${model.displayName}`}>测试</button>
                <RowMenu label={`更多操作 ${model.displayName}`} items={[
                  { key: 'copy', label: '复制模型 ID', onSelect: () => void navigator.clipboard?.writeText(model.upstreamId).then(() => setNotice(`已复制 ${model.upstreamId}`), () => setError('剪贴板不可用。')) },
                  {
                    key: 'catalog', label: model.inCatalog ? '移出 Codex 目录' : '纳入 Codex 目录',
                    onSelect: () => setConfirm({
                      title: model.inCatalog ? '移出 Codex 目录' : '纳入 Codex 目录',
                      body: model.inCatalog
                        ? `「${model.displayName}」将从待应用目录中移除；需要重新生成差异并应用后，Codex 菜单才会更新。`
                        : `「${model.displayName}」将纳入待应用目录；保存后需要重新生成差异并应用到 Codex。`,
                      label: model.inCatalog ? '移出目录' : '纳入目录',
                      run: async () => { await client.saveModel(modelDraft(model, { inCatalog: !model.inCatalog }), model.version); await finish('已更新目录归属。'); },
                    }),
                  },
                  {
                    key: 'delete', label: '删除模型', danger: true, disabled: model.inCatalog,
                    hint: model.inCatalog ? '先移出目录才能删除' : undefined,
                    onSelect: () => setConfirm({
                      title: '删除模型',
                      body: `将删除「${model.displayName}」（上游 ID ${model.upstreamId}）。此操作不可撤销。`,
                      label: '删除模型', danger: true,
                      run: async () => { await client.deleteModel(model.id, model.version); await finish('已删除模型。'); },
                    }),
                  },
                ]} />
              </div></td>
            </tr>)}</tbody>
          </table>
        </div>
        <p className={styles.count}>{visible.length} 项{visible.length !== models.length ? `（共 ${models.length} 项）` : ''}</p>
      </section>}

    {probe && <Dialog title={`测试 ${probe.model.displayName}`} busy={busy === 'probe'}
      description="只读探测：只读取上游模型列表，不会发起生成请求，因此不产生费用。"
      onClose={() => setProbe(null)}>
      <div className="form-fields">
        <ul className={styles.stages}>{probe.stages.map(stage => <li key={stage.stageKey} className={styles[stage.status] ?? ''}>
          <strong>{stage.stageKey}</strong>
          <span>{stage.status === 'passed' ? '通过' : stage.status === 'failed' ? '失败' : '跳过'}</span>
          <span className="text-muted">{stage.messageKey}</span>
          {stage.elapsedMs != null && <span className="text-mono text-muted">{stage.elapsedMs} ms</span>}
        </li>)}</ul>
        <div className="actions" style={{ justifyContent: 'flex-end' }}>
          <button onClick={() => setProbe(null)} autoFocus>关闭</button>
        </div>
      </div>
    </Dialog>}

    {confirm && <Dialog title={confirm.title} description={confirm.body} onClose={() => setConfirm(null)} busy={busy !== ''}>
      <div className="form-fields"><div className="form-footer">
        <span>此操作不可撤销。</span>
        <div className="actions">
          <button onClick={() => setConfirm(null)} disabled={busy !== ''}>取消</button>
          <button className={confirm.danger ? 'danger' : 'primary'} autoFocus disabled={busy !== ''} onClick={async () => {
            const action = confirm; setConfirm(null); await run('bulk', action.run);
          }}>{confirm.label}</button>
        </div>
      </div></div>
    </Dialog>}
  </div>;
}
