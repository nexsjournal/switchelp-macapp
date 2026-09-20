import { render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';

import { ModelsPage } from './ModelsPage';
import { ToastHost } from '@/components/Toast';
import type { Model, Provider } from '@/contracts/types';
import { testClient } from '../../../tests/helpers/client';
import { renderWithToasts } from '../../../tests/helpers/render';

/**
 * 模型列表页。
 *
 * 这个页面过去完全没有测试，而它承担的是「批量改目录归属」这类会直接写回 Codex 配置的
 * 操作：筛选、排序、全选、批量移出/删除、批量中途失败后的刷新。
 */

const providerA: Provider = {
  id: 'p_a', name: '供应商甲', endpoint: 'https://a.example.test/v1', protocol: 'responses',
  authKind: 'api_key', activeCredentialId: 'k_1', enabled: true, version: 1,
  createdAt: '2026-09-18T00:00:00Z', updatedAt: '2026-09-18T00:00:00Z',
};
const providerB: Provider = { ...providerA, id: 'p_b', name: '供应商乙', activeCredentialId: null };

function model(id: string, providerId: string, displayName: string, overrides: Partial<Model> = {}): Model {
  return {
    id, providerId, upstreamId: `vendor/${id}`, catalogAlias: `gs/${id}`, displayName,
    lifecycle: 'saved', hostState: 'pending_apply', inCatalog: true,
    policy: {
      contextLimit: 128_000, outputLimit: 8_192, compactLimit: null,
      reasoning: { support: 'supported', control: 'effort', allowedValues: ['low'], defaultValue: 'low', budgetTokens: null, mappingId: 'm' },
      inputs: [], tools: { functionTools: 'supported', parallelTools: 'unknown', customTools: 'unsupported', verification: 'declared' },
    },
    displayNameLayer: { discovered: null, userValue: displayName, overridden: true },
    capabilityRevision: 1, version: 1, createdAt: '2026-09-18T00:00:00Z', updatedAt: '2026-09-18T00:00:00Z',
    ...overrides,
  };
}

const catalogModel = model('m_1', 'p_a', '目录中的模型');
const looseModel = model('m_2', 'p_b', '目录外的模型', { inCatalog: false, hostState: 'not_in_catalog' });

function renderPage(overrides: Partial<Parameters<typeof testClient>[0]> = {}, models: Model[] = [catalogModel, looseModel]) {
  const client = testClient(overrides);
  const onChanged = vi.fn();
  const onEditModel = vi.fn();
  renderWithToasts(<ModelsPage client={client} providers={[providerA, providerB]} models={models} onChanged={onChanged} onEditModel={onEditModel} />);
  return { client, onChanged, onEditModel };
}

describe('模型列表', () => {
  it('按名称搜索、按供应商与目录归属筛选', async () => {
    const user = userEvent.setup();
    renderPage();

    expect(screen.getByText('目录中的模型')).toBeInTheDocument();
    expect(screen.getByText('目录外的模型')).toBeInTheDocument();

    await user.type(screen.getByLabelText('搜索模型'), 'vendor/m_2');
    expect(screen.queryByText('目录中的模型')).not.toBeInTheDocument();
    expect(screen.getByText('目录外的模型')).toBeInTheDocument();
    await user.clear(screen.getByLabelText('搜索模型'));

    await user.selectOptions(screen.getByLabelText('供应商筛选'), 'p_b');
    expect(screen.queryByText('目录中的模型')).not.toBeInTheDocument();
    expect(screen.getByText('目录外的模型')).toBeInTheDocument();
    await user.selectOptions(screen.getByLabelText('供应商筛选'), 'all');

    await user.selectOptions(screen.getByLabelText('目录筛选'), 'in_catalog');
    expect(screen.getByText('目录中的模型')).toBeInTheDocument();
    expect(screen.queryByText('目录外的模型')).not.toBeInTheDocument();
  });

  it('全选只覆盖当前可见行，批量移出要先确认', async () => {
    const user = userEvent.setup();
    const saveModel = vi.fn().mockResolvedValue(catalogModel);
    const { onChanged } = renderPage({ saveModel });

    await user.click(screen.getByLabelText('全选模型'));
    const bar = screen.getByRole('region', { name: '批量操作' });
    expect(within(bar).getByText(/已选 2 个 · 跨 2 家供应商/)).toBeInTheDocument();

    await user.click(within(bar).getByRole('button', { name: '批量移出目录' }));
    const dialog = await screen.findByRole('dialog');
    // 已在目录外的模型不会被移出，确认框要说清可执行的数量。
    expect(within(dialog).getByText(/其中可执行 1 个/)).toBeInTheDocument();
    expect(saveModel).not.toHaveBeenCalled();

    await user.click(within(dialog).getByRole('button', { name: '移出目录' }));
    expect(saveModel).toHaveBeenCalledWith(expect.objectContaining({ id: 'm_1', inCatalog: false }), 1);
    expect(onChanged).toHaveBeenCalled();
    // 提交后清空选择，避免下一次批量操作带上刚处理过的行。
    expect(screen.queryByRole('region', { name: '批量操作' })).not.toBeInTheDocument();
  });

  it('批量中途失败也要刷新列表，并说明完成了多少', async () => {
    const user = userEvent.setup();
    const saveModel = vi.fn()
      .mockResolvedValueOnce(catalogModel)
      .mockRejectedValueOnce(new Error('第二个失败'));
    const { onChanged } = renderPage({ saveModel }, [
      model('m_1', 'p_a', '甲'),
      model('m_2', 'p_a', '乙'),
    ]);

    await user.click(screen.getByLabelText('全选模型'));
    await user.click(within(screen.getByRole('region', { name: '批量操作' })).getByRole('button', { name: '批量移出目录' }));
    await user.click(within(await screen.findByRole('dialog')).getByRole('button', { name: '移出目录' }));

    // 回归：任由异常冒出去会导致列表不刷新——界面上看不到已经成功的那一条，
    // 用户会以为整批都没生效。
    // 部分完成会要求人处理剩下的，属于失败档：提示不自动消失（role=alert）。
    expect(await screen.findByRole('alert')).toHaveTextContent(/已完成 1 \/ 2 项/);
    expect(onChanged).toHaveBeenCalled();
  });

  it('目录中的模型不能直接删，先要移出', async () => {
    const user = userEvent.setup();
    const { client } = renderPage();

    await user.click(screen.getByRole('button', { name: '更多操作 目录中的模型' }));
    const item = await screen.findByRole('menuitem', { name: '删除模型' });
    // 菜单项是原生 button：禁用而不是藏起来，并给出原因（title）。
    expect(item).toBeDisabled();
    expect(item).toHaveAttribute('title', '先移出目录才能删除');
    expect(client.deleteModel).not.toHaveBeenCalled();

    // 目录外的模型可以直接删。
    await user.click(screen.getByRole('button', { name: '更多操作 目录外的模型' }));
    await user.click(await screen.findByRole('menuitem', { name: '删除模型' }));
    const dialog = await screen.findByRole('dialog');
    expect(within(dialog).getAllByText(/此操作不可撤销/).length).toBeGreaterThan(0);
  });

  it('供应商没有可用 Key 时说明无法测试，而不是静默失败', async () => {
    const user = userEvent.setup();
    renderPage({ listCredentials: vi.fn().mockResolvedValue([]) });

    await user.click(screen.getByRole('button', { name: '测试 目录中的模型' }));
    expect(await screen.findByRole('alert')).toHaveTextContent('还没有可用的 Key，无法测试');
  });
});

describe('宿主给定的供应商作用域', () => {
  /**
   * 回归：作用域曾经被复制进 `useState(providerScope ?? 'all')`，而切换供应商不会重挂载本组件，
   * 于是标题已经换成 B、表格里还是 A 的模型，B 的模型在界面上完全没有入口。
   */
  const client = testClient();
  const scoped = (providerScope: string) =>
    <><ModelsPage client={client} providers={[providerA, providerB]} models={[catalogModel, looseModel]}
      onChanged={vi.fn()} onEditModel={vi.fn()} providerScope={providerScope} embedded /><ToastHost /></>;

  it('切换作用域后表格内容跟着换，勾选不跨供应商保留', async () => {
    const user = userEvent.setup();
    const { rerender } = render(scoped('p_a'));
    expect(screen.getByText('目录中的模型')).toBeInTheDocument();
    expect(screen.queryByText('目录外的模型')).not.toBeInTheDocument();

    // 在 A 上勾一个，再去 B：那个勾不能跟过去，否则批量删除会作用在看不见的行上。
    await user.click(screen.getByLabelText('选择 目录中的模型'));
    expect(screen.getByRole('region', { name: '批量操作' })).toBeInTheDocument();

    rerender(scoped('p_b'));

    expect(screen.getByText('目录外的模型')).toBeInTheDocument();
    expect(screen.queryByText('目录中的模型')).not.toBeInTheDocument();
    expect(screen.queryByRole('region', { name: '批量操作' })).not.toBeInTheDocument();
  });
});

describe('编辑入口', () => {
  it('编辑与添加都上报给宿主打开编辑器，本页不再内置编辑器', async () => {
    const user = userEvent.setup();
    const { onEditModel } = renderPage();

    await user.click(screen.getByRole('button', { name: '编辑 目录中的模型' }));
    expect(onEditModel).toHaveBeenCalledWith(catalogModel);

    await user.click(screen.getByRole('button', { name: '添加模型' }));
    expect(onEditModel).toHaveBeenCalledWith('new');
    // 页面自始至终没有渲染过编辑器标题。
    expect(screen.queryByRole('heading', { level: 1, name: '新增模型' })).not.toBeInTheDocument();
  });
});
