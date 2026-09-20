import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { vi } from 'vitest';
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

const renderEditor = (overrides = {}, props = {}) => render(<ModelEditorPage client={testClient(overrides)} providers={[provider]}
  model={model} onSaved={() => {}} onCancel={() => {}} {...props} />);

test('是独立页面：分组标题、滚动区里的字段，页尾只有取消与保存', () => {
  renderEditor();

  expect(screen.getByRole('heading', { level: 1, name: '代码模型' })).toBeInTheDocument();
  expect(screen.getByRole('button', { name: /模型/ })).toBeInTheDocument();
  // 一页只留一个主按钮：不再有「保存草稿」和「保存并查看应用差异」两条几乎一样的路。
  expect(screen.getByRole('button', { name: '保存' })).toBeInTheDocument();
  expect(screen.queryByRole('button', { name: '保存草稿' })).not.toBeInTheDocument();
  expect(screen.queryByRole('button', { name: '保存并查看应用差异' })).not.toBeInTheDocument();
  expect(screen.getByRole('button', { name: '取消' })).toBeInTheDocument();
  // 生效预览整块撤掉：同样的信息挂在每个字段的「?」上，不再单开一栏让人对照着读。
  expect(screen.queryByRole('complementary', { name: '生效预览' })).not.toBeInTheDocument();
  expect(screen.getByText(/保存后到「Codex 配置」页生成差异并应用/)).toBeInTheDocument();
});

test('输入类型与模型能力是勾选单元格：文本锁定，PDF 与视频不可启用', () => {
  renderEditor();

  expect(screen.getByRole('checkbox', { name: '文本' })).toBeDisabled();
  expect(screen.getByRole('checkbox', { name: '文本' })).toBeChecked();
  expect(screen.getByRole('checkbox', { name: 'PDF' })).toBeDisabled();
  expect(screen.getByRole('checkbox', { name: '视频' })).toBeDisabled();
  expect(screen.getByRole('checkbox', { name: '图片' })).toBeEnabled();
  // 三态下拉没有了：勾＝支持、不勾＝不支持，没点过的项保存时原样保留。
  expect(screen.queryByRole('combobox', { name: '文本' })).not.toBeInTheDocument();
  expect(screen.getByRole('checkbox', { name: '函数工具' })).toBeChecked();
  expect(screen.getByRole('checkbox', { name: '并行工具' })).not.toBeChecked();
});

test('勾上图片、取消函数工具后按版本号提交', async () => {
  const user = userEvent.setup();
  const saveModel = vi.fn().mockResolvedValue(model);
  renderEditor({ saveModel });

  await user.click(screen.getByRole('checkbox', { name: '图片' }));
  await user.click(screen.getByRole('checkbox', { name: '函数工具' }));
  await user.click(screen.getByRole('button', { name: '保存' }));

  expect(saveModel).toHaveBeenCalledTimes(1);
  const draft = saveModel.mock.calls[0]![0]!;
  expect(saveModel.mock.calls[0]![1]).toBe(4);
  expect(draft.policy.inputs.find((entry: { kind: string }) => entry.kind === 'image')?.upstream).toBe('supported');
  expect(draft.policy.tools.functionTools).toBe('unsupported');
});

test('没动过思考那一节时，已保存的「开关式」声明原样保留', async () => {
  const user = userEvent.setup();
  const saveModel = vi.fn().mockResolvedValue(model);
  const toggleModel = { ...model, policy: { ...model.policy,
    reasoning: { support: 'supported' as const, control: 'toggle' as const, allowedValues: [], defaultValue: null, budgetTokens: null, mappingId: null } } };
  render(<ModelEditorPage client={testClient({ saveModel })} providers={[provider]} model={toggleModel}
    onSaved={() => {}} onCancel={() => {}} />);

  // 档位 chip 只表达档位式；这一节没被碰过，就得原样留着，不能因为打开一次页面就改写。
  expect(screen.getByText(/已保存的声明是「开启 \/ 关闭」式思考/)).toBeInTheDocument();
  await user.type(screen.getByLabelText('显示名称'), '改个名');
  await user.click(screen.getByRole('button', { name: '保存' }));

  expect(saveModel.mock.calls[0]![0]!.policy.reasoning.control).toBe('toggle');
});

test('推理档位：加号添加、点一下设为默认，保存后进策略', async () => {
  const user = userEvent.setup();
  const saveModel = vi.fn().mockResolvedValue(model);
  renderEditor({ saveModel });

  // 已保存的档位是 low / high，默认 low；再加一个 minimal 并把它设为默认。
  for (const level of ['minimal', 'high']) {
    await user.click(screen.getByRole('button', { name: '添加档位' }));
    await user.type(screen.getByLabelText('添加档位'), `${level}{Enter}`);
  }
  await user.click(screen.getByRole('button', { name: 'minimal' }));
  await user.click(screen.getByRole('button', { name: '保存' }));

  const reasoning = saveModel.mock.calls[0]![0]!.policy.reasoning;
  // 重复的档位不会加第二遍。
  expect(reasoning.allowedValues).toEqual(['low', 'high', 'minimal']);
  expect(reasoning.defaultValue).toBe('minimal');
  expect(reasoning.mappingId).toBe('reasoning.effort.v1');
});

test('有未保存修改时取消要确认，避免一次点击丢掉填写', async () => {
  const user = userEvent.setup();
  const onCancel = vi.fn();
  renderEditor({}, { onCancel });

  await user.type(screen.getByLabelText('显示名称'), '改一下');
  await user.click(screen.getByRole('button', { name: '取消' }));

  expect(onCancel).not.toHaveBeenCalled();
  expect(await screen.findByText('有尚未保存的修改，确定放弃吗？')).toBeInTheDocument();
  await user.click(screen.getByRole('button', { name: '放弃修改' }));
  expect(onCancel).toHaveBeenCalled();
});

test('没有修改时取消直接返回', async () => {
  const user = userEvent.setup();
  const onCancel = vi.fn();
  renderEditor({}, { onCancel });

  await user.click(screen.getByRole('button', { name: '取消' }));
  expect(onCancel).toHaveBeenCalled();
});
