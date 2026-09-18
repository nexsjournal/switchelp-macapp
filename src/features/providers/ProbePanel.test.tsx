import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { ProbePanel } from './ProbePanel';
import { provider, testClient } from '../../../tests/helpers/client';

function probeReport(stages: { stageKey: string; status: 'passed' | 'failed' | 'skipped'; messageKey: string }[]) {
  return { id: 'probe_1', targetLabel: '测试供应商', stages, startedAt: '2026-09-18T00:00:00Z' };
}

test('测试连接只读探测：逐阶段显示结论，不宣称整体成功', async () => {
  const user = userEvent.setup();
  const client = testClient({
    startProbe: vi.fn().mockResolvedValue(probeReport([
      { stageKey: 'connect', status: 'passed', messageKey: 'probe.connected' },
      { stageKey: 'credential', status: 'passed', messageKey: 'probe.credentialAccepted' },
      { stageKey: 'model', status: 'failed', messageKey: 'probe.modelMissing' },
    ])),
  });
  render(<ProbePanel client={client} provider={provider} credentialId="k_1" />);

  await user.click(screen.getByRole('button', { name: '测试连接' }));

  expect(await screen.findByText('上游模型列表里没有这个 ID')).toBeInTheDocument();
  expect(screen.getByText('失败')).toBeInTheDocument();
  expect(screen.getByText('探测未全部通过，见下方阶段明细。')).toBeInTheDocument();
  // 默认必须只读，不能悄悄产生费用。
  expect(client.startProbe).toHaveBeenCalledWith(
    { providerId: 'p_test', credentialId: 'k_1' },
    { includeGenerate: false },
  );
  expect(screen.queryByText(/发起了真实请求/)).not.toBeInTheDocument();
});

test('勾选真实请求后才标注副作用并透传该选项', async () => {
  const user = userEvent.setup();
  const client = testClient({
    startProbe: vi.fn().mockResolvedValue({
      ...probeReport([{ stageKey: 'generate', status: 'passed', messageKey: 'probe.generatePassed' }]),
      generated: true,
    }),
  });
  render(<ProbePanel client={client} provider={provider} credentialId="k_1" />);

  await user.click(screen.getByLabelText(/发一次真实请求/));
  await user.click(screen.getByRole('button', { name: '测试连接' }));

  expect(await screen.findByText(/本次探测向供应商发起了真实请求/)).toBeInTheDocument();
  expect(client.startProbe).toHaveBeenCalledWith(
    { providerId: 'p_test', credentialId: 'k_1' },
    { includeGenerate: true },
  );
});

test('没有选中 Key 时不给测试入口', () => {
  render(<ProbePanel client={testClient()} provider={provider} credentialId={null} />);

  expect(screen.getByText('先添加并选择一个 Key，再测试连接。')).toBeInTheDocument();
  expect(screen.getByRole('button', { name: '测试连接' })).toBeDisabled();
  expect(screen.getByRole('button', { name: /从上游获取模型/ })).toBeDisabled();
});
