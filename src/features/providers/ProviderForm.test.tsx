import { screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';

import { ProviderForm } from './ProviderForm';
import type { Credential, Model, Provider } from '@/contracts/types';
import { testClient } from '../../../tests/helpers/client';
import { renderWithToasts } from '../../../tests/helpers/render';

/**
 * 供应商弹窗的「常用供应商预设」与连接测试行。
 *
 * 预设是接入路径的第一步：点一家把名称、地址、协议填好，唯一留给用户的是 API Key。
 * 这里验证三件事——模板真的填进表单、来源（presetId）真的写进保存调用、
 * 编辑既有供应商时预设不再出现。
 */

const savedProvider: Provider = {
  id: 'p_saved', name: '测试供应商', endpoint: 'https://example.test/v1', protocol: 'responses',
  authKind: 'api_key', activeCredentialId: 'k_1', enabled: true, version: 1,
  createdAt: '2026-09-18T00:00:00Z', updatedAt: '2026-09-18T00:00:00Z',
};

const credential: Credential = {
  id: 'k_1', providerId: 'p_saved', label: '日常', secretRef: 'gptswitch/p_saved/k_1/v1', secretVersion: 1,
  maskedSuffix: '••••0001', status: 'verified', scope: null, lastVerifiedAt: null, version: 1,
  createdAt: '2026-09-18T00:00:00Z',
};

function model(id: string, providerId: string, displayName: string): Model {
  return {
    id, providerId, upstreamId: `vendor/${id}`, catalogAlias: `gs/${id}`, displayName,
    lifecycle: 'saved', hostState: 'loaded', inCatalog: true,
    policy: {
      contextLimit: 128_000, outputLimit: 8_192, compactLimit: null,
      reasoning: { support: 'supported', control: 'effort', allowedValues: ['low'], defaultValue: 'low', budgetTokens: null, mappingId: 'm' },
      inputs: [], tools: { functionTools: 'supported', parallelTools: 'unknown', customTools: 'unsupported', builtinTools: 'unsupported', verification: 'declared' },
    },
    displayNameLayer: { discovered: null, userValue: displayName, overridden: true },
    capabilityRevision: 1, version: 1, createdAt: '2026-09-18T00:00:00Z', updatedAt: '2026-09-18T00:00:00Z',
  };
}

function renderForm(overrides: Parameters<typeof testClient>[0] = {}, props: { provider?: Provider; models?: Model[] } = {}) {
  const client = testClient({
    listCredentials: vi.fn().mockResolvedValue(props.provider ? [credential] : []),
    saveProvider: vi.fn().mockImplementation(async (draft: Record<string, unknown>) => ({
      ...savedProvider, ...draft, id: savedProvider.id, version: 1,
    })),
    addCredential: vi.fn().mockResolvedValue(credential),
    selectCredential: vi.fn().mockResolvedValue(undefined),
    startProbe: vi.fn().mockResolvedValue({
      id: 'probe_test', targetLabel: 'test', startedAt: '2026-09-18T00:00:00Z',
      stages: [{ stageKey: 'connect', status: 'passed', elapsedMs: 5, messageKey: 'probe.connected' }],
    }),
    ...overrides,
  });
  const onClose = vi.fn();
  renderWithToasts(<ProviderForm client={client} provider={props.provider} providers={props.provider ? [props.provider] : []}
    models={props.models ?? []} onSaved={vi.fn()} onKeysChanged={vi.fn()} onChanged={vi.fn()} onClose={onClose} />);
  return { client, onClose };
}

describe('常用供应商预设', () => {
  it('新建态列出常用供应商；点 DeepSeek 填好名称、地址与协议，Key 留空并聚焦', async () => {
    const user = userEvent.setup();
    renderForm();

    const dialog = screen.getByRole('dialog');
    expect(within(dialog).getByText('智谱 GLM')).toBeInTheDocument();
    expect(within(dialog).getByLabelText('API Key')).toHaveValue('');

    const chip = within(dialog).getByRole('button', { name: 'DeepSeek' });
    await user.click(chip);

    expect(within(dialog).getByLabelText('供应商名称')).toHaveValue('DeepSeek');
    expect(within(dialog).getByLabelText('Base URL')).toHaveValue('https://api.deepseek.com/v1');
    expect(within(dialog).getByLabelText('API 格式')).toHaveValue('chat_completions');
    expect(within(dialog).getByLabelText('API Key')).toHaveValue('');
    expect(within(dialog).getByLabelText('API Key')).toHaveFocus();
    expect(chip).toHaveAttribute('aria-pressed', 'true');
  });

  it('点「自定义」清空模板字段，从空白开始', async () => {
    const user = userEvent.setup();
    renderForm();
    const dialog = screen.getByRole('dialog');

    await user.click(within(dialog).getByRole('button', { name: 'DeepSeek' }));
    expect(within(dialog).getByLabelText('供应商名称')).toHaveValue('DeepSeek');

    await user.click(within(dialog).getByRole('button', { name: '自定义' }));
    expect(within(dialog).getByLabelText('供应商名称')).toHaveValue('');
    expect(within(dialog).getByLabelText('Base URL')).toHaveValue('');
    expect(within(dialog).getByRole('button', { name: '自定义' })).toHaveAttribute('aria-pressed', 'true');
  });

  it('Ollama 预设是本机服务：没有 Key 输入框，保存为无认证', async () => {
    const user = userEvent.setup();
    const { client, onClose } = renderForm();
    const dialog = screen.getByRole('dialog');

    await user.click(within(dialog).getByRole('button', { name: 'Ollama（本机）' }));
    expect(within(dialog).queryByLabelText('API Key')).not.toBeInTheDocument();

    await user.click(within(dialog).getByRole('button', { name: '保存' }));
    expect(client.saveProvider).toHaveBeenCalledWith(expect.objectContaining({
      name: 'Ollama（本机）', endpoint: 'http://localhost:11434/v1', authKind: 'none', presetId: 'ollama',
    }), 0);
    expect(client.addCredential).not.toHaveBeenCalled();
    await screen.findByText('已保存。');
    expect(onClose).toHaveBeenCalled();
  });

  it('从预设保存时把来源（presetId）与端点一起写进核心', async () => {
    const user = userEvent.setup();
    const { client } = renderForm();
    const dialog = screen.getByRole('dialog');

    await user.click(within(dialog).getByRole('button', { name: 'MiniMax' }));
    await user.type(within(dialog).getByLabelText('API Key'), 'synthetic-secret');
    await user.click(within(dialog).getByRole('button', { name: '保存' }));

    expect(client.saveProvider).toHaveBeenCalledWith(expect.objectContaining({
      name: 'MiniMax', endpoint: 'https://api.minimaxi.com/v1', protocol: 'chat_completions', presetId: 'minimax',
    }), 0);
    expect(client.addCredential).toHaveBeenCalledWith(expect.anything(), expect.anything(), 'synthetic-secret');
  });

  it('编辑既有供应商时不出现预设行', () => {
    renderForm({}, { provider: savedProvider });
    expect(screen.queryByText('常用供应商')).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: '自定义' })).not.toBeInTheDocument();
  });
});

describe('连接测试行', () => {
  it('还没有模型时按钮禁用，原因写在 title 里', () => {
    renderForm({}, { provider: savedProvider });
    const button = screen.getByRole('button', { name: '测试连接' });
    expect(button).toBeDisabled();
    expect(button).toHaveAttribute('title', '先添加至少一个模型，再测试连接。');
  });

  it('有模型时对第一个模型跑只读探测，Key 与模型 ID 来自当前供应商', async () => {
    const user = userEvent.setup();
    const { client } = renderForm({}, { provider: savedProvider, models: [model('m_1', 'p_saved', 'alpha 模型'), model('m_2', 'p_saved', 'beta 模型')] });

    await user.click(screen.getByRole('button', { name: '测试连接' }));

    // 列表按显示名排序，「第一个」是排序后的第一行（alpha），不是数组里的第一条。
    expect(client.startProbe).toHaveBeenCalledWith(
      { providerId: 'p_saved', modelId: 'm_1', credentialId: 'k_1' },
      { includeGenerate: false });
    expect(await screen.findByText('测试供应商 / alpha 模型 连接成功')).toBeInTheDocument();
  });
});
