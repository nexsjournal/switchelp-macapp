import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { vi } from 'vitest';

import { ModelFormDialog } from './ModelFormDialog';
import { DISCOVERY_DEFAULT_LIMITS } from './policy';
import { testClient } from '../../../tests/helpers/client';

/**
 * 「添加模型」弹窗：供应商弹窗里的高频加模型路径。
 * 这里钉住四件事：最小必填集与默认长度、智能配置关掉之后的语义、
 * 高级配置的折叠与不可启用项、档位集合怎么落进策略。
 */

test('手动添加：显示名沿用模型 ID，智能配置补上推荐的默认长度', async () => {
  const user = userEvent.setup();
  const saveModel = vi.fn().mockResolvedValue({});
  const onSaved = vi.fn().mockResolvedValue(undefined);
  render(<ModelFormDialog client={testClient({ saveModel })} providerId="p_test" onSaved={onSaved} onClose={() => {}} />);

  await user.type(screen.getByLabelText('模型 ID'), 'vendor/manual');
  await user.click(screen.getByRole('button', { name: '保存' }));

  expect(saveModel).toHaveBeenCalledTimes(1);
  const draft = saveModel.mock.calls[0]![0]!;
  expect(draft).toMatchObject({ providerId: 'p_test', upstreamId: 'vendor/manual', displayName: 'vendor/manual', inCatalog: true });
  expect(draft.policy.contextLimit).toBe(DISCOVERY_DEFAULT_LIMITS.contextLimit);
  expect(draft.policy.outputLimit).toBe(DISCOVERY_DEFAULT_LIMITS.outputLimit);
  expect(onSaved).toHaveBeenCalled();
});

test('长度手填时按 128k 解析，不再被默认值顶掉', async () => {
  const user = userEvent.setup();
  const saveModel = vi.fn().mockResolvedValue({});
  render(<ModelFormDialog client={testClient({ saveModel })} providerId="p_test" onSaved={() => {}} onClose={() => {}} />);

  await user.type(screen.getByLabelText('模型 ID'), 'vendor/manual');
  await user.type(screen.getByLabelText('上下文窗口'), '256k');
  await user.type(screen.getByLabelText('最大输出 Token'), '16k');
  await user.click(screen.getByRole('button', { name: '保存' }));

  const policy = saveModel.mock.calls[0]![0]!.policy;
  expect(policy.contextLimit).toBe(256_000);
  expect(policy.outputLimit).toBe(16_000);
});

test('关掉智能配置后，留空的长度是「未声明」而不是默认值', async () => {
  const user = userEvent.setup();
  const saveModel = vi.fn().mockResolvedValue({});
  render(<ModelFormDialog client={testClient({ saveModel })} providerId="p_test" onSaved={() => {}} onClose={() => {}} />);

  await user.click(screen.getByRole('switch', { name: '智能配置' }));
  await user.type(screen.getByLabelText('模型 ID'), 'vendor/manual');
  await user.click(screen.getByRole('button', { name: '保存' }));

  const policy = saveModel.mock.calls[0]![0]!.policy;
  expect(policy.contextLimit).toBeNull();
  expect(policy.outputLimit).toBeNull();
});

test('高级配置默认折叠；文本锁定，链路不支持的输入不可启用', () => {
  render(<ModelFormDialog client={testClient()} providerId="p_test" onSaved={() => {}} onClose={() => {}} />);

  const details = screen.getByText('高级配置').closest('details');
  expect(details).not.toBeNull();
  expect(details).not.toHaveAttribute('open');
  details!.querySelector('summary')!.click();
  expect(details).toHaveAttribute('open');

  // 文本是链路底线：可见、勾着、不能取消。
  expect(screen.getByRole('checkbox', { name: '文本' })).toBeDisabled();
  expect(screen.getByRole('checkbox', { name: '文本' })).toBeChecked();
  // PDF 与视频当前链路发不出去：可见但不可启用，不是藏起来。
  expect(screen.getByRole('checkbox', { name: 'PDF' })).toBeDisabled();
  expect(screen.getByRole('checkbox', { name: '视频' })).toBeDisabled();
  expect(screen.getByRole('checkbox', { name: '图片' })).toBeEnabled();
});

test('推理档位：加两个档位后写进策略，默认取第一个', async () => {
  const user = userEvent.setup();
  const saveModel = vi.fn().mockResolvedValue({});
  render(<ModelFormDialog client={testClient({ saveModel })} providerId="p_test" onSaved={() => {}} onClose={() => {}} />);

  screen.getByText('高级配置').closest('details')!.querySelector('summary')!.click();
  await user.type(screen.getByLabelText('模型 ID'), 'vendor/manual');
  for (const level of ['low', 'high']) {
    await user.click(screen.getByRole('button', { name: '添加档位' }));
    await user.type(screen.getByLabelText('添加档位'), `${level}{Enter}`);
  }
  await user.click(screen.getByRole('button', { name: '保存' }));

  const reasoning = saveModel.mock.calls[0]![0]!.policy.reasoning;
  expect(reasoning.allowedValues).toEqual(['low', 'high']);
  expect(reasoning.defaultValue).toBe('low');
  expect(reasoning.mappingId).toBe('reasoning.effort.v1');
});

test('重置表单清掉已填内容，恢复默认的智能配置', async () => {
  const user = userEvent.setup();
  render(<ModelFormDialog client={testClient()} providerId="p_test" onSaved={() => {}} onClose={() => {}} />);

  await user.type(screen.getByLabelText('模型 ID'), 'vendor/manual');
  await user.click(screen.getByRole('switch', { name: '智能配置' }));
  expect(screen.getByRole('switch', { name: '智能配置' })).toHaveAttribute('aria-checked', 'false');

  await user.click(screen.getByRole('button', { name: '重置表单' }));
  expect(screen.getByLabelText('模型 ID')).toHaveValue('');
  expect(screen.getByRole('switch', { name: '智能配置' })).toHaveAttribute('aria-checked', 'true');
});
