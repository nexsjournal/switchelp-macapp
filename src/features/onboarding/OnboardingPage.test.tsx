import { render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { OnboardingPage } from './OnboardingPage';
import { instance, provider, testClient } from '../../../tests/helpers/client';

function setup(overrides: Partial<Parameters<typeof OnboardingPage>[0]> = {}) {
  const handlers = { onOpenProviderForm: vi.fn(), onOpenModelEditor: vi.fn(), onViewDiff: vi.fn(), onDismiss: vi.fn() };
  render(<OnboardingPage client={testClient({ detectInstances: vi.fn().mockResolvedValue([instance]) })}
    providers={[]} models={[]} credentialsByProvider={{}} {...handlers} {...overrides} />);
  return handlers;
}

test('三步可返回，步骤指示器标出当前步', async () => {
  const user = userEvent.setup();
  setup();

  expect(await screen.findByText('检测 Codex 实例')).toBeInTheDocument();
  const stepList = screen.getByRole('list', { name: '接入步骤' });
  expect(within(stepList).getAllByRole('listitem')).toHaveLength(3);
  expect(within(stepList).getByText('检测 Codex').closest('li')).toHaveAttribute('aria-current', 'step');
  expect(screen.getByRole('button', { name: /上一步/ })).toBeDisabled();

  await user.click(screen.getByRole('button', { name: /下一步/ }));
  // 步骤指示器里也有同名文案，这里断言卡片标题。
  expect(screen.getByRole('heading', { name: '添加供应商和模型' })).toBeInTheDocument();

  await user.click(screen.getByRole('button', { name: /上一步/ }));
  expect(screen.getByText('检测 Codex 实例')).toBeInTheDocument();
});

test('检测不到时给安装指引与手动定位，不提供下载', async () => {
  setup({ client: testClient({ detectInstances: vi.fn().mockResolvedValue([]) }) });

  expect(await screen.findByText('未检测到 Codex')).toBeInTheDocument();
  expect(screen.getByText(/不会自动下载或安装 Codex/)).toBeInTheDocument();
  expect(screen.getByLabelText('Codex 应用路径')).toBeInTheDocument();
  // 界面不提供任何“下载 Codex”的动作。
  expect(screen.queryByRole('button', { name: /下载|安装 Codex/ })).not.toBeInTheDocument();
});

test('多个实例必须由用户选择，不自动挑一个', async () => {
  const second = { ...instance, id: 'inst_other', configFile: '/tmp/other/.codex/config.toml' };
  setup({ client: testClient({ detectInstances: vi.fn().mockResolvedValue([instance, second]) }) });

  expect(await screen.findByText(/请自己选择要用哪一个/)).toBeInTheDocument();
  const radios = screen.getAllByRole('radio');
  expect(radios).toHaveLength(2);
  expect(radios.every(radio => !(radio as HTMLInputElement).checked)).toBe(true);
});

test('检测到其他配置管理工具时说明冲突：残留是什么、在哪个文件、删之前先备份', async () => {
  setup({ client: testClient({ detectInstances: vi.fn().mockResolvedValue([{ ...instance, conflictingManagers: ['other-tool'] }]) }) });

  expect(await screen.findByText(/检测到其他配置管理工具：other-tool/)).toBeInTheDocument();
  // 只报一个工具名等于把「去哪儿删、删什么」留给用户猜。
  expect(screen.getByText(/托管标记还在配置文件里/)).toBeInTheDocument();
  const conflict = within(screen.getByRole('note'));
  expect(conflict.getByText('涉及的文件')).toBeInTheDocument();
  expect(conflict.getByText(instance.configFile)).toBeInTheDocument();
  expect(conflict.getByText(/删之前先备份文件/)).toBeInTheDocument();
});

test('第二步的清单反映真实数据，未满足条件时说明缺什么', async () => {
  const user = userEvent.setup();
  setup();

  await screen.findByText('检测 Codex 实例');
  await user.click(screen.getByRole('button', { name: /下一步/ }));

  expect(screen.getByText('还没有供应商')).toBeInTheDocument();
  expect(screen.getByText(/还需要在供应商页把某个 Key 设为当前/)).toBeInTheDocument();
  expect(screen.getByText(/需要至少一个「已选 Key 的供应商」/)).toBeInTheDocument();
});

test('跳过测试时明说不会被标成已验证', async () => {
  const user = userEvent.setup();
  setup();

  await screen.findByText('检测 Codex 实例');
  await user.click(screen.getByRole('button', { name: /下一步/ }));
  await user.click(screen.getByRole('button', { name: /下一步/ }));

  expect(screen.getByText('尚未确认 Codex 已加载新目录')).toBeInTheDocument();
  await user.click(screen.getByRole('button', { name: '跳过测试' }));

  expect(screen.getByText(/这个模型不会被显示成已验证/)).toBeInTheDocument();
  expect(screen.queryByText('只读检查通过')).not.toBeInTheDocument();
});

test('完成页是等待状态并给出具体模型名，不是庆祝页', async () => {
  const user = userEvent.setup();
  const model = { id: 'm_1', providerId: 'p_test', upstreamId: 'vendor/a', catalogAlias: 'gs/m_1', displayName: '代码模型',
    lifecycle: 'saved' as const, hostState: 'pending_apply' as const, inCatalog: true,
    policy: { contextLimit: 128_000, outputLimit: 8_192, compactLimit: null,
      reasoning: { support: 'unknown' as const, control: 'none' as const, allowedValues: [], defaultValue: null, budgetTokens: null, mappingId: null },
      inputs: [], tools: { functionTools: 'unknown' as const, parallelTools: 'unknown' as const, customTools: 'unknown' as const, verification: 'declared' as const } },
    displayNameLayer: { discovered: null, userValue: null, overridden: false },
    capabilityRevision: 1, version: 1, createdAt: '2026-09-18T00:00:00Z', updatedAt: '2026-09-18T00:00:00Z' };
  const handlers = setup({ providers: [provider], models: [model], credentialsByProvider: { p_test: [] } });

  await screen.findByText('检测 Codex 实例');
  await user.click(screen.getByRole('button', { name: /下一步/ }));
  await user.click(screen.getByRole('button', { name: /下一步/ }));

  expect(screen.getByText('在 Codex 模型选择器中选择')).toBeInTheDocument();
  expect(screen.getByText(/本次将纳入 1 个模型：代码模型/)).toBeInTheDocument();
  expect(screen.getByText('尚未确认 Codex 已加载新目录')).toBeInTheDocument();

  await user.click(screen.getByRole('button', { name: /查看差异并应用/ }));
  expect(handlers.onViewDiff).toHaveBeenCalled();
});

test('可以随时退出向导', async () => {
  const user = userEvent.setup();
  const handlers = setup();

  await screen.findByText('检测 Codex 实例');
  await user.click(screen.getByRole('button', { name: '稍后再说' }));
  expect(handlers.onDismiss).toHaveBeenCalled();
});
