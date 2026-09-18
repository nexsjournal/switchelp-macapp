import { render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { ModelEditorPage } from './ModelEditorPage';
import { provider, testClient } from '../../../tests/helpers/client';

const model = {
  id: 'm_1', providerId: 'p_test', upstreamId: 'vendor/model-x', catalogAlias: 'gs/m_1',
  displayName: '代码模型', lifecycle: 'saved' as const, hostState: 'pending_apply' as const, inCatalog: true,
  policy: { contextLimit: 128_000, outputLimit: 8_192, compactLimit: null,
    reasoning: { support: 'supported' as const, control: 'effort' as const, allowedValues: ['low', 'high'], defaultValue: 'low', budgetTokens: null, mappingId: 'reasoning.effort.v1' },
    inputs: [], tools: { functionTools: 'supported' as const, parallelTools: 'unknown' as const, customTools: 'unsupported' as const, verification: 'declared' as const } },
  displayNameLayer: { discovered: null, userValue: '代码模型', overridden: true },
  capabilityRevision: 1, version: 4, createdAt: '2026-09-18T00:00:00Z', updatedAt: '2026-09-18T00:00:00Z',
};

test('是独立页面：有面包屑、生效预览与页尾双按钮', async () => {
  render(<ModelEditorPage client={testClient()} providers={[provider]} model={model} onSaved={() => {}} onCancel={() => {}} />);

  expect(screen.getByRole('heading', { level: 1, name: '代码模型' })).toBeInTheDocument();
  expect(screen.getByRole('button', { name: /模型/ })).toBeInTheDocument();
  const preview = screen.getByRole('complementary', { name: '生效预览' });
  // 多个字段都落在 Codex 目录，所以这里断言“至少出现一次”。
  expect(within(preview).getAllByText('Codex 目录').length).toBeGreaterThan(0);
  expect(within(preview).getAllByText('网关请求').length).toBeGreaterThan(0);
  expect(screen.getByRole('button', { name: '保存草稿' })).toBeInTheDocument();
  expect(screen.getByRole('button', { name: '保存并查看应用差异' })).toBeInTheDocument();
});

test('生效预览说明每项参数最终落在哪里，而不是重复标签', () => {
  render(<ModelEditorPage client={testClient()} providers={[provider]} model={model} onSaved={() => {}} onCancel={() => {}} />);

  const preview = screen.getByRole('complementary', { name: '生效预览' });
  expect(within(preview).getByText(/决定 Codex 何时压缩历史/)).toBeInTheDocument();
  expect(within(preview).getByText(/任何一层不支持就不会出现在原生能力里/)).toBeInTheDocument();
});

test('保存并查看应用差异会先保存再上报跳转意图', async () => {
  const user = userEvent.setup();
  const saveModel = vi.fn().mockResolvedValue(model);
  const onSaved = vi.fn().mockResolvedValue(undefined);
  const onViewDiff = vi.fn();
  render(<ModelEditorPage client={testClient({ saveModel })} providers={[provider]} model={model}
    onSaved={onSaved} onCancel={() => {}} onViewDiff={onViewDiff} />);

  await user.click(screen.getByRole('button', { name: '保存并查看应用差异' }));

  expect(saveModel).toHaveBeenCalledWith(expect.objectContaining({ id: 'm_1', upstreamId: 'vendor/model-x' }), 4);
  expect(onSaved).toHaveBeenCalled();
  expect(onViewDiff).toHaveBeenCalled();
});

test('只保存草稿时不上报跳转', async () => {
  const user = userEvent.setup();
  const onViewDiff = vi.fn();
  render(<ModelEditorPage client={testClient({ saveModel: vi.fn().mockResolvedValue(model) })} providers={[provider]} model={model}
    onSaved={() => {}} onCancel={() => {}} onViewDiff={onViewDiff} />);

  await user.click(screen.getByRole('button', { name: '保存草稿' }));
  expect(onViewDiff).not.toHaveBeenCalled();
});


test('校验没过时点“保存并查看差异”，不会污染下一次“保存草稿”', async () => {
  const user = userEvent.setup();
  const onViewDiff = vi.fn();
  render(<ModelEditorPage client={testClient({ saveModel: vi.fn().mockResolvedValue(model) })} providers={[provider]} model={model}
    onSaved={() => {}} onCancel={() => {}} onViewDiff={onViewDiff} />);

  // 清空必填的显示名称，再点“保存并查看应用差异”：浏览器会拦下提交。
  await user.clear(screen.getByLabelText('显示名称'));
  await user.click(screen.getByRole('button', { name: '保存并查看应用差异' }));
  expect(onViewDiff).not.toHaveBeenCalled();

  // 补全后点“保存草稿”：提交意图来自当前按钮，不应该被上一次的点击带偏。
  await user.type(screen.getByLabelText('显示名称'), '补上的名字');
  await user.click(screen.getByRole('button', { name: '保存草稿' }));
  expect(onViewDiff).not.toHaveBeenCalled();

  // 反过来：真的点“保存并查看应用差异”时仍然要跳转。
  await user.click(screen.getByRole('button', { name: '保存并查看应用差异' }));
  expect(onViewDiff).toHaveBeenCalledTimes(1);
});

test('有未保存修改时取消要确认，避免一次点击丢掉填写', async () => {
  const user = userEvent.setup();
  const onCancel = vi.fn();
  render(<ModelEditorPage client={testClient()} providers={[provider]} model={model} onSaved={() => {}} onCancel={onCancel} />);

  await user.type(screen.getByLabelText(/显示名称/), '改一下');
  await user.click(screen.getByRole('button', { name: '取消' }));

  expect(onCancel).not.toHaveBeenCalled();
  expect(await screen.findByText('有尚未保存的修改，确定放弃吗？')).toBeInTheDocument();
  await user.click(screen.getByRole('button', { name: '放弃修改' }));
  expect(onCancel).toHaveBeenCalled();
});

test('没有修改时取消直接返回', async () => {
  const user = userEvent.setup();
  const onCancel = vi.fn();
  render(<ModelEditorPage client={testClient()} providers={[provider]} model={model} onSaved={() => {}} onCancel={onCancel} />);

  await user.click(screen.getByRole('button', { name: '取消' }));
  expect(onCancel).toHaveBeenCalled();
});
