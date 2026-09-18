import { render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { App } from './App';
import { provider, testClient } from '../../tests/helpers/client';

test('首次接入保存真实草稿调用，失败后保留表单且不宣称 Codex 已加载', async () => {
  const user = userEvent.setup();
  const client = testClient({ saveProvider: vi.fn().mockRejectedValue({ code: 'VALIDATION_FAILED', messageKey: 'error.validation',
    safeDetails: ['远程地址必须使用 HTTPS'], retryable: false, recoveryActions: [] }) });
  render(<App client={client} />);
  await screen.findByText('添加第一个供应商');
  await user.click(screen.getAllByRole('button', { name: '添加供应商' })[0]!);
  const dialog = screen.getByRole('dialog');
  await user.type(within(dialog).getByLabelText('供应商名称'), '测试服务');
  await user.type(within(dialog).getByLabelText('API 地址'), 'http://example.test/v1');
  await user.click(within(dialog).getByRole('button', { name: '保存供应商' }));
  expect(await within(dialog).findByRole('alert')).toHaveTextContent('HTTPS');
  expect(within(dialog).getByLabelText('供应商名称')).toHaveValue('测试服务');
  expect(client.saveProvider).toHaveBeenCalledWith(expect.objectContaining({ name: '测试服务', endpoint: 'http://example.test/v1', protocol: 'responses' }), 0);
  expect(client.executeApply).not.toHaveBeenCalled();
  expect(screen.getByText('尚未应用到 Codex')).toBeInTheDocument();
});

test('编辑脏表单按 Escape 需要确认，放弃后焦点返回入口', async () => {
  const user = userEvent.setup();
  render(<App client={testClient()} />);
  await screen.findByText('添加第一个供应商');
  const trigger = screen.getAllByRole('button', { name: '添加供应商' })[0]!;
  await user.click(trigger);
  await user.type(screen.getByLabelText('供应商名称'), '草稿');
  await user.keyboard('{Escape}');
  expect(screen.getByText('有尚未保存的修改，确定放弃吗？')).toBeInTheDocument();
  await user.click(screen.getByRole('button', { name: '继续编辑' }));
  expect(screen.getByLabelText('供应商名称')).toHaveValue('草稿');
  await user.keyboard('{Escape}');
  await user.click(screen.getByRole('button', { name: '放弃修改' }));
  expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
  await waitFor(() => expect(trigger).toHaveFocus());
});

test('保存 Key 只通过专用调用传递并清空密码框，不自动切换', async () => {
  const user = userEvent.setup();
  const addCredential = vi.fn().mockRejectedValue({ code: 'KEYSTORE_LOCKED', messageKey: 'error.keystoreLocked',
    safeDetails: ['系统凭据库不可用'], retryable: true, recoveryActions: [] });
  const client = testClient({ listProviders: vi.fn().mockResolvedValue({ items: [provider], nextCursor: null }), addCredential });
  render(<App client={client} />);
  await screen.findByText('测试供应商');
  await user.click(within(screen.getByRole('navigation')).getByRole('button', { name: '供应商' }));
  await user.click(screen.getByRole('button', { name: '添加 Key' }));
  await user.type(screen.getByLabelText('Key 备注'), '日常');
  await user.type(screen.getByLabelText('API Key'), 'synthetic-secret');
  await user.click(screen.getByRole('button', { name: '安全保存' }));
  expect(addCredential).toHaveBeenCalledWith('p_test', '日常', 'synthetic-secret');
  expect(screen.getByLabelText('API Key')).toHaveValue('');
  expect(await screen.findByRole('alert')).toHaveTextContent('系统凭据库不可用');
  expect(client.selectCredential).not.toHaveBeenCalled();
  expect(localStorage.length).toBe(0);
});

test('网关未启动时明确显示原因，不显示成已接通或已应用', async () => {
  const client = testClient({ gatewayStatus: vi.fn().mockResolvedValue({ running: false, port: null, served: 0,
    revisions: [], tokenFingerprint: '', error: '无法绑定 127.0.0.1:18765：地址已被占用' }) });
  render(<App client={client} />);

  expect(await screen.findByText('网关未启动')).toBeInTheDocument();
  expect(await screen.findByText('网关未启动：无法绑定 127.0.0.1:18765：地址已被占用')).toBeInTheDocument();
  expect(screen.queryByText('已应用到 Codex')).not.toBeInTheDocument();
});

test('网关运行中且未发布目录时，不宣称已应用到 Codex', async () => {
  const client = testClient({ gatewayStatus: vi.fn().mockResolvedValue({ running: true, port: 18765, served: 0,
    revisions: [], tokenFingerprint: 'deadbeef', error: null }) });
  render(<App client={client} />);

  expect(await screen.findByText(/网关运行中 · 127.0.0.1:18765 · 尚未发布目录/)).toBeInTheDocument();
  expect(screen.queryByText('已应用到 Codex')).not.toBeInTheDocument();
  expect(screen.getByText('尚未应用到 Codex')).toBeInTheDocument();
});

test('纳入目录的模型不能直接删除，必须先移出', async () => {
  const model = { id: 'm_1', providerId: 'p_test', upstreamId: 'vendor/a', catalogAlias: 'gs/m_1',
    displayName: '目录中的模型', lifecycle: 'saved' as const, hostState: 'pending_apply' as const, inCatalog: true,
    policy: { contextLimit: 128_000, outputLimit: 8_192, compactLimit: null,
      reasoning: { support: 'unknown' as const, control: 'none' as const, allowedValues: [], defaultValue: null, budgetTokens: null, mappingId: null },
      inputs: [], tools: { functionTools: 'unknown' as const, parallelTools: 'unknown' as const, customTools: 'unknown' as const, verification: 'declared' as const } },
    displayNameLayer: { discovered: null, userValue: null, overridden: false }, capabilityRevision: 1, version: 3,
    createdAt: '2026-09-18T00:00:00Z', updatedAt: '2026-09-18T00:00:00Z' };
  const client = testClient({ listProviders: vi.fn().mockResolvedValue({ items: [provider], nextCursor: null }),
    listModels: vi.fn().mockResolvedValue([model]) });
  const user = userEvent.setup();
  render(<App client={client} />);
  await screen.findByText('目录中的模型');
  await user.click(within(screen.getByRole('navigation')).getByRole('button', { name: '模型' }));
  await user.click(await screen.findByRole('button', { name: '更多操作 目录中的模型' }));

  // 菜单里能看到删除，但已纳入目录时禁用。
  expect(screen.getByRole('menuitem', { name: '删除模型' })).toBeDisabled();
  expect(screen.getByRole('menuitem', { name: '移出 Codex 目录' })).toBeEnabled();
});

test('移出目录要确认，并按版本号提交 inCatalog=false', async () => {
  const user = userEvent.setup();
  const saveModel = vi.fn().mockResolvedValue({});
  const model = { id: 'm_1', providerId: 'p_test', upstreamId: 'vendor/a', catalogAlias: 'gs/m_1',
    displayName: '目录中的模型', lifecycle: 'saved' as const, hostState: 'pending_apply' as const, inCatalog: true,
    policy: { contextLimit: 128_000, outputLimit: 8_192, compactLimit: null,
      reasoning: { support: 'unknown' as const, control: 'none' as const, allowedValues: [], defaultValue: null, budgetTokens: null, mappingId: null },
      inputs: [], tools: { functionTools: 'unknown' as const, parallelTools: 'unknown' as const, customTools: 'unknown' as const, verification: 'declared' as const } },
    displayNameLayer: { discovered: null, userValue: null, overridden: false }, capabilityRevision: 1, version: 3,
    createdAt: '2026-09-18T00:00:00Z', updatedAt: '2026-09-18T00:00:00Z' };
  const client = testClient({ listProviders: vi.fn().mockResolvedValue({ items: [provider], nextCursor: null }),
    listModels: vi.fn().mockResolvedValue([model]), saveModel });
  render(<App client={client} />);
  await screen.findByText('目录中的模型');
  await user.click(within(screen.getByRole('navigation')).getByRole('button', { name: '模型' }));
  await user.click(await screen.findByRole('button', { name: '更多操作 目录中的模型' }));
  await user.click(screen.getByRole('menuitem', { name: '移出 Codex 目录' }));
  const dialog = await screen.findByRole('dialog');
  expect(within(dialog).getByText(/需要重新生成差异并应用/)).toBeInTheDocument();
  expect(saveModel).not.toHaveBeenCalled();

  await user.click(within(dialog).getByRole('button', { name: '移出目录' }));
  expect(saveModel).toHaveBeenCalledWith(expect.objectContaining({ id: 'm_1', inCatalog: false }), 3);
});

test('删除 Key 会说明撤销凭据库条目，并只在确认后调用', async () => {
  const user = userEvent.setup();
  const deleteCredential = vi.fn().mockResolvedValue(undefined);
  const credential = { id: 'k_1', providerId: 'p_test', label: '备用', secretRef: 'r', secretVersion: 1,
    maskedSuffix: '••••1c7', status: 'saved' as const, scope: null, lastVerifiedAt: null, version: 1,
    createdAt: '2026-09-18T00:00:00Z' };
  const client = testClient({ listProviders: vi.fn().mockResolvedValue({ items: [provider], nextCursor: null }),
    listCredentials: vi.fn().mockResolvedValue([credential]), deleteCredential });
  render(<App client={client} />);
  await screen.findByText('测试供应商');
  await user.click(within(screen.getByRole('navigation')).getByRole('button', { name: '供应商' }));
  await user.click(await screen.findByRole('button', { name: '删除 备用' }));

  const dialog = await screen.findByRole('dialog');
  expect(within(dialog).getByText(/撤销系统凭据库里的条目/)).toBeInTheDocument();
  expect(deleteCredential).not.toHaveBeenCalled();

  await user.click(within(dialog).getByRole('button', { name: '删除 Key' }));
  expect(deleteCredential).toHaveBeenCalledWith('k_1');
});
