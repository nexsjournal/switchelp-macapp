import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { ModelForm } from './ModelForm';
import { provider, testClient } from '../../../tests/helpers/client';
import { parseTokens } from './policy';

test('自定义模型保存精确 token 数、输入声明、思考集合，不自动应用', async () => {
  const user = userEvent.setup();
  const saveModel = vi.fn().mockResolvedValue({});
  const onSaved = vi.fn();
  const client = testClient({ saveModel });
  render(<ModelForm client={client} providers={[provider]} onSaved={onSaved} onClose={vi.fn()} />);
  await user.type(screen.getByLabelText('显示名称'), '我的模型');
  await user.type(screen.getByLabelText('上游模型 ID'), 'Vendor/Model-X');
  await user.type(screen.getByLabelText('上下文窗口'), '128k');
  await user.type(screen.getByLabelText('最大输出'), '8,192');
  await user.selectOptions(screen.getByLabelText('PDF'), 'supported');
  await user.selectOptions(screen.getByLabelText('是否支持'), 'supported');
  await user.type(screen.getByLabelText('支持的取值'), 'low, high');
  await user.type(screen.getByLabelText('默认取值'), 'high');
  await user.click(screen.getByRole('button', { name: '保存模型' }));
  expect(saveModel).toHaveBeenCalledWith(expect.objectContaining({ upstreamId: 'Vendor/Model-X', inCatalog: true,
    policy: expect.objectContaining({ contextLimit: 128000, outputLimit: 8192,
      reasoning: expect.objectContaining({ allowedValues: ['low', 'high'], defaultValue: 'high' }),
      inputs: expect.arrayContaining([expect.objectContaining({ kind: 'pdf', upstream: 'supported', effectivePath: 'blocked' })]) }) }), 0);
  expect(onSaved).toHaveBeenCalledOnce();
  expect(client.executeApply).not.toHaveBeenCalled();
});

test('错误的输出预算在提交前阻止保存', async () => {
  const user = userEvent.setup();
  const client = testClient();
  render(<ModelForm client={client} providers={[provider]} onSaved={vi.fn()} onClose={vi.fn()} />);
  await user.type(screen.getByLabelText('显示名称'), '模型');
  await user.type(screen.getByLabelText('上游模型 ID'), 'x');
  await user.type(screen.getByLabelText('上下文窗口'), '8k');
  await user.type(screen.getByLabelText('最大输出'), '32k');
  await user.click(screen.getByRole('button', { name: '保存模型' }));
  expect(await screen.findByRole('alert')).toHaveTextContent('最大输出必须小于上下文窗口');
  expect(client.saveModel).not.toHaveBeenCalled();
});

test('Token 单位解析无歧义并拒绝零、负数和溢出', () => {
  expect(parseTokens('32k')).toBe(32000);
  expect(parseTokens('32Ki')).toBe(32768);
  expect(parseTokens('')).toBeNull();
  for (const input of ['0', '-1', '1.5k', '99999999999', 'NaN']) expect(() => parseTokens(input)).toThrow();
});
