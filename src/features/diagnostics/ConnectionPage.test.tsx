import { act, render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';

import { ConnectionPage } from './ConnectionPage';
import type { Credential, Model, ProbeResult, Provider } from '@/contracts/types';
import { testClient } from '../../../tests/helpers/client';

/**
 * 连接诊断页。
 *
 * 之前没有任何测试，于是两个真实缺陷一直没被发现：首屏不加载模型与 Key（下拉空着、
 * 开始按钮点不了，还不说明原因），以及快速切换供应商时旧响应会覆盖新选择。
 */

const provider = (id: string, name: string, active: string | null = null): Provider => ({
  id, name, endpoint: `https://${id}.example.test/v1`, protocol: 'responses', authKind: 'api_key',
  activeCredentialId: active, enabled: true, version: 1,
  createdAt: '2026-09-18T00:00:00Z', updatedAt: '2026-09-18T00:00:00Z',
});

const model = (id: string, providerId: string, displayName: string): Model => ({
  id, providerId, upstreamId: `vendor/${id}`, catalogAlias: `gs/${id}`, displayName, lifecycle: 'saved',
  hostState: 'loaded', inCatalog: true,
  policy: {
    contextLimit: 128_000, outputLimit: 8_192, compactLimit: null,
    reasoning: { support: 'supported', control: 'effort', allowedValues: ['low', 'high'], defaultValue: 'low', budgetTokens: null, mappingId: 'm' },
    inputs: [], tools: { functionTools: 'supported', parallelTools: 'unknown', customTools: 'unsupported', builtinTools: 'unsupported', verification: 'declared' },
  },
  displayNameLayer: { discovered: null, userValue: displayName, overridden: true },
  capabilityRevision: 1, version: 1, createdAt: '2026-09-18T00:00:00Z', updatedAt: '2026-09-18T00:00:00Z',
});

const credential = (id: string, providerId: string, label: string): Credential => ({
  id, providerId, label, secretRef: `gptswitch/${providerId}/${id}/v1`, secretVersion: 1,
  maskedSuffix: '••••4f2a', status: 'verified', scope: null, lastVerifiedAt: null, version: 1,
  createdAt: '2026-09-18T00:00:00Z',
});

const report = (): ProbeResult => ({
  id: 'probe_1',
  targetLabel: 'gs/m_a',
  startedAt: '2026-09-18T00:00:00Z',
  generated: false,
  stages: [
    { stageKey: 'connect', status: 'passed', messageKey: 'probe.connected', elapsedMs: 12 },
    { stageKey: 'credential', status: 'failed', messageKey: 'probe.timedOut', elapsedMs: 30 },
  ],
});

describe('连接诊断页', () => {
  it('首屏就加载所选供应商的模型与 Key，不需要用户先动下拉', async () => {
    const listModels = vi.fn().mockResolvedValue([model('m_a', 'p_a', '模型甲'), model('m_b', 'p_b', '模型乙')]);
    const listCredentials = vi.fn().mockResolvedValue([credential('k_1', 'p_a', '日常')]);
    render(<ConnectionPage client={testClient({ listModels, listCredentials })} providers={[provider('p_a', '供应商甲', 'k_1')]} />);

    // 模型与 Key 下拉应被填好，开始按钮可用。
    const modelSelect = screen.getByLabelText('选择模型');
    // 选项文本是「显示名 · 上游 ID」。
    expect(await within(modelSelect).findByRole('option', { name: /模型甲/ })).toBeInTheDocument();
    expect(within(modelSelect).queryByRole('option', { name: /模型乙/ })).not.toBeInTheDocument();
    expect(within(screen.getByLabelText('选择 Key')).getByRole('option', { name: '日常 ••••4f2a' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: '开始检查' })).toBeEnabled();
    expect(listCredentials).toHaveBeenCalledWith('p_a');
  });

  it('切换供应商时后返回的旧响应不会覆盖新选择', async () => {
    const user = userEvent.setup();
    // 甲的凭据慢、乙的快：先选甲再立刻选乙，最终必须停在乙的数据上。
    let releaseA: (value: Credential[]) => void = () => {};
    const slow = new Promise<Credential[]>(resolve => { releaseA = resolve; });
    const listCredentials = vi.fn((id: string) => id === 'p_a'
      ? slow
      : Promise.resolve([credential('k_b', 'p_b', '乙的 Key')]));
    render(<ConnectionPage client={testClient({
      listModels: vi.fn().mockResolvedValue([model('m_a', 'p_a', '模型甲'), model('m_b', 'p_b', '模型乙')]),
      listCredentials,
    })} providers={[provider('p_a', '供应商甲'), provider('p_b', '供应商乙')]} />);

    // 先选慢的甲，没等它回来就切到快的乙。
    await user.selectOptions(screen.getByLabelText('选择供应商'), 'p_a');
    await user.selectOptions(screen.getByLabelText('选择供应商'), 'p_b');
    expect(await screen.findByRole('option', { name: '乙的 Key ••••4f2a' })).toBeInTheDocument();

    releaseA([credential('k_a', 'p_a', '甲的 Key')]);
    // 等被丢弃的响应真正回写状态后再断言：否则断言可能早于 React 重渲染，竞态测不出来。
    await act(async () => { await slow; });
    expect(screen.queryByRole('option', { name: '甲的 Key ••••4f2a' })).not.toBeInTheDocument();
  });

  it('失败阶段给出可执行建议，并把内部键翻成文案', async () => {
    const user = userEvent.setup();
    const startProbe = vi.fn().mockResolvedValue(report());
    render(<ConnectionPage client={testClient({
      listModels: vi.fn().mockResolvedValue([model('m_a', 'p_a', '模型甲')]),
      listCredentials: vi.fn().mockResolvedValue([credential('k_1', 'p_a', '日常')]),
      startProbe,
    })} providers={[provider('p_a', '供应商甲', 'k_1')]} />);

    await within(screen.getByLabelText('选择模型')).findByRole('option', { name: /模型甲/ });
    await user.click(screen.getByRole('button', { name: '开始检查' }));

    expect(await screen.findByText('阶段时间线')).toBeInTheDocument();
    // 阶段名与状态都走文案层，不能把 connect/passed 这类内部值摆出来。
    expect(screen.getByText('连接')).toBeInTheDocument();
    expect(screen.getByText('通过')).toBeInTheDocument();
    // probe.timedOut 的建议复用「连接超时」标题与步骤（阶段副文案与建议列表都会用到）。
    expect(screen.getAllByText('连接超时').length).toBeGreaterThan(0);
    expect(screen.getAllByText(/如果走代理，检查代理是否在运行/).length).toBeGreaterThan(0);
    // 可复制的诊断摘要要包含阶段结论与建议步骤，且是纯文本。
    const summary = (screen.getByLabelText('诊断摘要') as HTMLTextAreaElement).value;
    expect(summary).toContain('连接超时');
    expect(summary).toContain('如果走代理，检查代理是否在运行');
    expect(summary).not.toContain('probe.timedOut');
    // 只读检查默认不发真实请求。
    expect(startProbe).toHaveBeenCalledWith(expect.objectContaining({ providerId: 'p_a', modelId: 'm_a' }), { includeGenerate: false });
  });

  it('没有供应商时说明先做什么，而不是留一个点不动的按钮', () => {
    render(<ConnectionPage client={testClient()} providers={[]} />);
    expect(screen.getByText(/先在供应商页添加一个供应商和 Key/)).toBeInTheDocument();
    // 下拉仍在，但明确写着「没有可选的」，开始按钮不可点。
    expect(within(screen.getByLabelText('选择供应商')).getByRole('option', { name: '（还没有供应商）' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: '开始检查' })).toBeDisabled();
  });
});
