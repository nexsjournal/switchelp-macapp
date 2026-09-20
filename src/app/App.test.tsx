import { render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { App } from './App';
import { instance, plan, provider, testClient } from '../../tests/helpers/client';
import type { Credential, Provider } from '@/contracts/types';

test('首次接入保存真实草稿调用，失败后保留表单且不宣称 Codex 已加载', async () => {
  const user = userEvent.setup();
  const client = testClient({ saveProvider: vi.fn().mockRejectedValue({ code: 'VALIDATION_FAILED', messageKey: 'error.validation',
    safeDetails: ['远程地址必须使用 HTTPS'], retryable: false, recoveryActions: [] }) });
  render(<App client={client} />);
  // 没有供应商时会先进接入向导；这里要测的是供应商表单，先退出向导。
  await user.click(await screen.findByRole('button', { name: '稍后再说' }));
  await screen.findByText('添加第一个供应商');
  await user.click(screen.getAllByRole('button', { name: '添加供应商' })[0]!);
  const dialog = screen.getByRole('dialog');
  await user.type(within(dialog).getByLabelText('供应商名称'), '测试服务');
  await user.type(within(dialog).getByLabelText('Base URL'), 'http://example.test/v1');
  // 添加时 API Key 必填：不留 Key 的供应商一建出来就停在「待填写 Key」上。
  await user.type(within(dialog).getByLabelText('API Key'), 'synthetic-secret');
  await user.click(within(dialog).getByRole('button', { name: '保存' }));
  expect(await screen.findByRole('alert')).toHaveTextContent('HTTPS');
  expect(within(dialog).getByLabelText('供应商名称')).toHaveValue('测试服务');
  expect(client.saveProvider).toHaveBeenCalledWith(expect.objectContaining({ name: '测试服务', endpoint: 'http://example.test/v1' }), 0);
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

test('回归：保存供应商不会因为「刚把 Key 设为当前」而误报冲突', async () => {
  // 核心按版本号拒绝覆盖别人的修改，而「把新 Key 设为当前 Key」这一步本身就会把供应商的版本推高
  // （保存 v1 → 选当前 Key → v2）。弹窗如果继续拿创建时那份 v1 去写，用户看到的就是一句
  // 既没原因也没出路的「保存失败，请刷新后重试」——这正是真机上发生的事。
  const user = userEvent.setup();
  let version = 0;
  const stored = () => ({ ...provider, id: 'p_new', name: '示例中转', version, activeCredentialId: version > 1 ? 'k_new' : null });
  const saveProvider = vi.fn().mockImplementation(async (draft: Record<string, unknown>, expected: number) => {
    if (expected !== version) throw { code: 'CONFLICT', messageKey: 'error.conflict', safeDetails: [], retryable: false, recoveryActions: [] };
    version += 1;
    return { ...stored(), ...draft, id: draft.id ?? 'p_new' };
  });
  const selectCredential = vi.fn().mockImplementation(async () => { version += 1; });
  const client = testClient({
    listProviders: vi.fn().mockImplementation(async () => ({ items: version ? [stored()] : [], nextCursor: null })),
    saveProvider, selectCredential,
    addCredential: vi.fn().mockResolvedValue({ id: 'k_new', label: '默认' }),
    listCredentials: vi.fn().mockResolvedValue([]),
    discoverModels: vi.fn().mockResolvedValue([{ upstreamId: 'vendor/a', displayName: 'a', alreadySaved: false }]),
  });
  render(<App client={client} />);
  await user.click(within(screen.getByRole('navigation')).getByRole('button', { name: '供应商与模型' }));
  await user.click(screen.getAllByRole('button', { name: '添加供应商' })[0]!);
  const dialog = screen.getByRole('dialog');
  await user.type(within(dialog).getByLabelText('供应商名称'), '示例中转');
  await user.type(within(dialog).getByLabelText('Base URL'), 'https://api.relay.example.test/v1');
  await user.type(within(dialog).getByLabelText('API Key'), 'synthetic-secret');

  // 「获取可用模型」会先落库，并把 Key 设为当前 Key —— 版本因此从 1 变成 2。
  await user.click(within(dialog).getByRole('button', { name: '获取可用模型' }));
  await user.click(within(await screen.findByRole('dialog', { name: '选择要添加的模型' })).getByRole('button', { name: /取消/ }));

  await user.click(within(dialog).getByRole('button', { name: '保存' }));
  expect(await screen.findByText('已保存。')).toBeInTheDocument();
  expect(screen.queryByText('保存失败，请刷新后重试。')).not.toBeInTheDocument();
  expect(saveProvider).toHaveBeenLastCalledWith(expect.objectContaining({ name: '示例中转' }), 2);
});

test('回归：宿主的版本还没刷新上来时，撞上冲突会自己重读再写一次', async () => {
  const user = userEvent.setup();
  const saveProvider = vi.fn()
    .mockRejectedValueOnce({ code: 'CONFLICT', messageKey: 'error.conflict', safeDetails: [], retryable: false, recoveryActions: [] })
    .mockImplementation(async (draft: Record<string, unknown>) => ({ ...provider, ...draft, version: 3 }));
  const active = { ...provider, activeCredentialId: 'k_1' };
  const credential = { id: 'k_1', providerId: 'p_test', label: '日常', secretRef: 'r', secretVersion: 1,
    maskedSuffix: '••••1c7', status: 'saved' as const, scope: null, lastVerifiedAt: null, version: 1,
    createdAt: '2026-09-18T00:00:00Z' };
  const client = testClient({
    // 打开页面时看到的还是 v1（宿主列表还没刷新），而库里已经是 v2 —— 冲突就是这么来的。
    // 重读时返回 v2，第二次写才能成功。
    listProviders: vi.fn()
      .mockResolvedValueOnce({ items: [{ ...active, version: 1 }], nextCursor: null })
      .mockResolvedValue({ items: [{ ...active, version: 2 }], nextCursor: null }),
    saveProvider, listCredentials: vi.fn().mockResolvedValue([credential]),
  });
  render(<App client={client} />);
  await user.click(within(screen.getByRole('navigation')).getByRole('button', { name: '供应商与模型' }));
  await user.click(screen.getByRole('button', { name: '编辑配置' }));
  const dialog = await screen.findByRole('dialog');

  await user.click(within(dialog).getByRole('button', { name: '保存' }));
  expect(await screen.findByText('已保存。')).toBeInTheDocument();
  // 第一次用旧版本被拒，第二次带着重读到的版本写成。
  expect(saveProvider).toHaveBeenNthCalledWith(1, expect.anything(), 1);
  expect(saveProvider).toHaveBeenNthCalledWith(2, expect.anything(), 2);
});

test('回归：配好模型后，页面上有「应用并重启 Codex」这一步，不必自己去找入口', async () => {
  // 真机反馈：在「供应商与模型」页配好供应商与模型之后，行上只有「编辑 / 测试 / 更多」，
  // 没有任何能让配置生效的按钮——用户只能猜。待应用条把这一步摆在页面上。
  const user = userEvent.setup();
  const model = modelRow();
  const planApply = vi.fn().mockResolvedValue(plan([{ keyPath: 'model', before: null, after: 'gs/m_1', reasonKey: 'reason.defaultModel' }]));
  const executeApply = vi.fn().mockResolvedValue({ operationId: 'op_1' });
  const restartHost = vi.fn().mockResolvedValue({ appPath: '/Applications/ChatGPT.app', quitConfirmed: true, quitForced: false, launchedConfirmed: true });
  const confirmReload = vi.fn().mockResolvedValue({ operationId: 'op_1', open: false, events: [] });
  const client = testClient({ listProviders: vi.fn().mockResolvedValue({ items: [provider], nextCursor: null }),
    listModels: vi.fn().mockResolvedValue([model]), listCredentials: vi.fn().mockResolvedValue([]),
    detectInstances: vi.fn().mockResolvedValue([instance]), planApply, executeApply, restartHost, confirmReload,
    applyStatus: vi.fn().mockResolvedValue({ operationId: 'op_1', open: false, events: [] }) });
  render(<App client={client} />);
  await screen.findByText('测试供应商');
  await user.click(within(screen.getByRole('navigation')).getByRole('button', { name: '供应商与模型' }));

  // 条上写清「几个模型待应用」以及为什么要重启。
  const bar = await screen.findByRole('region', { name: '待应用' });
  expect(within(bar).getByText('1 个模型待应用')).toBeInTheDocument();
  expect(within(bar).getByText(/Codex 只在启动时读配置/)).toBeInTheDocument();

  // 点「应用并重启」→ 先给差异确认（写的是 Codex 自己的配置文件，不能静默写）→ 确认后写入并重启。
  await user.click(within(bar).getByRole('button', { name: '应用并重启 Codex' }));
  const confirm = await screen.findByRole('dialog', { name: '应用差异' });
  expect(planApply).toHaveBeenCalled();
  expect(executeApply).not.toHaveBeenCalled();

  await user.click(within(confirm).getByRole('button', { name: '应用并重启 Codex' }));
  await waitFor(() => expect(executeApply).toHaveBeenCalledTimes(1));
  expect(restartHost).toHaveBeenCalledWith('inst_test');
  // 重启是本应用自己做的，而且退出与启动都观察到了——回执当场记下，不再让用户去
  // 「Codex 配置」页点第二次。以前不记账，事务永远停在「等待重载」。
  await waitFor(() => expect(confirmReload).toHaveBeenCalledWith('op_1', true));
  expect(await screen.findByText('配置已提交，Codex 已重启；它回来后看看模型菜单。')).toBeInTheDocument();
});

test('已应用但没等到回执时，条上说事实，并把回执交回用户', async () => {
  // 自动补记不成立的场合（宿主没重启、平台查不到启动时间）不能让用户卡在「待应用」上：
  // 那条会误导人再点一次「应用并重启」，而配置其实早就写好了。
  const user = userEvent.setup();
  const applied = { ...modelRow(), hostState: 'awaiting_reload' as const };
  const confirmReload = vi.fn().mockResolvedValue({ operationId: 'op_9', open: false, events: [] });
  const client = testClient({ listProviders: vi.fn().mockResolvedValue({ items: [provider], nextCursor: null }),
    listModels: vi.fn().mockResolvedValue([applied]), listCredentials: vi.fn().mockResolvedValue([]),
    applySummary: vi.fn().mockResolvedValue({ operationId: 'op_9', instanceId: 'inst_test', catalogRevision: 'rev_1',
      defaultModel: 'gs/m_1', aliasCount: 1, stage: 'awaiting_reload', appliedAt: '2026-09-20T00:00:00Z' }),
    confirmReload });
  render(<App client={client} />);
  await screen.findByText('测试供应商');
  await user.click(within(screen.getByRole('navigation')).getByRole('button', { name: '供应商与模型' }));

  const bar = await screen.findByRole('region', { name: '待应用' });
  expect(within(bar).getByText('已应用，等 Codex 确认加载')).toBeInTheDocument();
  expect(within(bar).queryByRole('button', { name: '应用并重启 Codex' })).toBeNull();

  await user.click(within(bar).getByRole('button', { name: 'Codex 已经重启了' }));
  await waitFor(() => expect(confirmReload).toHaveBeenCalledWith('op_9', true));
});

test('启动时对账：宿主已经重启过就自动补记，不必等用户去点', async () => {
  const reconcileReload = vi.fn().mockResolvedValue({ confirmedOperationIds: ['op_9'] });
  const listModels = vi.fn().mockResolvedValue([{ ...modelRow(), hostState: 'awaiting_reload' as const }]);
  const client = testClient({ listProviders: vi.fn().mockResolvedValue({ items: [provider], nextCursor: null }),
    listModels, listCredentials: vi.fn().mockResolvedValue([]), reconcileReload });
  render(<App client={client} />);

  await waitFor(() => expect(reconcileReload).toHaveBeenCalled());
  // 有事务被补记就重读数据：模型状态才会从「等待重载」翻过去。
  await waitFor(() => expect(listModels.mock.calls.length).toBeGreaterThan(1));
});

test('待应用条的「查看差异」交给宿主切到 Codex 配置页', async () => {
  const user = userEvent.setup();
  const client = testClient({ listProviders: vi.fn().mockResolvedValue({ items: [provider], nextCursor: null }),
    listModels: vi.fn().mockResolvedValue([modelRow()]), listCredentials: vi.fn().mockResolvedValue([]) });
  render(<App client={client} />);
  await screen.findByText('测试供应商');
  await user.click(within(screen.getByRole('navigation')).getByRole('button', { name: '供应商与模型' }));
  const bar = await screen.findByRole('region', { name: '待应用' });
  await user.click(within(bar).getByRole('button', { name: '查看差异' }));
  expect(await screen.findByRole('heading', { level: 1, name: 'Codex 配置' })).toBeInTheDocument();
});

test('供应商弹窗：标题是文字，名称是表单字段，「更多」里有停用与删除', async () => {
  const user = userEvent.setup();
  const deleteProvider = vi.fn().mockResolvedValue(undefined);
  const client = testClient({ listProviders: vi.fn().mockResolvedValue({ items: [provider], nextCursor: null }),
    listCredentials: vi.fn().mockResolvedValue([]), deleteProvider, listModels: vi.fn().mockResolvedValue([]) });
  render(<App client={client} />);
  await screen.findByText('测试供应商');
  await user.click(within(screen.getByRole('navigation')).getByRole('button', { name: '供应商与模型' }));
  await user.click(screen.getByRole('button', { name: '编辑配置' }));
  const dialog = await screen.findByRole('dialog');

  // 标题就是普通文字（以前它是个没有边框的输入框，没人看得出能点）；名字回到表单里。
  expect(within(dialog).getByRole('heading', { level: 2, name: '测试供应商' })).toBeInTheDocument();
  expect(within(dialog).getByLabelText('供应商名称')).toHaveValue('测试供应商');

  // 启用开关不再挂在标题栏；停用与删除都在菜单里。
  expect(within(dialog).queryByRole('switch', { name: '启用此供应商' })).not.toBeInTheDocument();
  await user.click(within(dialog).getByRole('button', { name: '更多操作' }));
  expect(await screen.findByRole('menuitem', { name: '停用此供应商' })).toBeInTheDocument();
  await user.click(screen.getByRole('menuitem', { name: '删除供应商' }));
  const confirm = await screen.findByRole('dialog', { name: '删除供应商' });
  await user.click(within(confirm).getByRole('button', { name: '删除供应商' }));
  await waitFor(() => expect(deleteProvider).toHaveBeenCalledWith('p_test'));
});

test('替换当前 Key 只通过专用调用传递秘密，不把掩码当原值提交', async () => {
  const user = userEvent.setup();
  const replaceCredential = vi.fn().mockRejectedValue({ code: 'KEYSTORE_LOCKED', messageKey: 'error.keystoreLocked',
    safeDetails: ['系统凭据库不可用'], retryable: true, recoveryActions: [] });
  const credential = { id: 'k_1', providerId: 'p_test', label: '日常', secretRef: 'r', secretVersion: 1,
    maskedSuffix: '••••1c7', status: 'saved' as const, scope: null, lastVerifiedAt: null, version: 2,
    createdAt: '2026-09-18T00:00:00Z' };
  const active = { ...provider, activeCredentialId: 'k_1' };
  const client = testClient({ listProviders: vi.fn().mockResolvedValue({ items: [active], nextCursor: null }),
    listCredentials: vi.fn().mockResolvedValue([credential]), replaceCredential,
    saveProvider: vi.fn().mockResolvedValue(active) });
  render(<App client={client} />);
  await screen.findByText('测试供应商');
  await user.click(within(screen.getByRole('navigation')).getByRole('button', { name: '供应商与模型' }));
  await user.click(screen.getByRole('button', { name: '编辑配置' }));
  const dialog = await screen.findByRole('dialog');
  // 已有 Key 时输入框是空的，占位符给出掩码：留空＝不动它，写新值＝替换。
  const field = within(dialog).getByLabelText('API Key');
  expect(field).toHaveValue('');
  expect(field).toHaveAttribute('placeholder', expect.stringContaining('••••1c7'));

  await user.type(field, 'synthetic-secret');
  await user.click(within(dialog).getByRole('button', { name: '保存' }));

  expect(replaceCredential).toHaveBeenCalledWith('k_1', 'synthetic-secret', 2);
  expect(client.addCredential).not.toHaveBeenCalled();
  expect(await screen.findByRole('alert')).toHaveTextContent('系统凭据库不可用');
  // 允许保存界面偏好（例如主题、向导是否已看过），但绝不允许出现秘密或 Key 明文。
  expect(JSON.stringify(localStorage)).not.toContain('synthetic-secret');
  expect(JSON.stringify(localStorage)).not.toContain('synthetic');
});

test('添加供应商时第一个 Key 一起保存并设为当前，弹窗留在原地继续用', async () => {
  const user = userEvent.setup();
  const saveProvider = vi.fn().mockResolvedValue({ ...provider, id: 'p_new', name: '新服务', activeCredentialId: 'k_new' });
  const addCredential = vi.fn().mockResolvedValue({ id: 'k_new', label: '默认' });
  const selectCredential = vi.fn().mockResolvedValue(undefined);
  const client = testClient({ saveProvider, addCredential, selectCredential });
  render(<App client={client} />);
  // 不依赖接入向导是否已展开：直接从导航进供应商页，用页头的「添加供应商」。
  await user.click(within(screen.getByRole('navigation')).getByRole('button', { name: '供应商与模型' }));
  // 空态和页头各有一个「添加供应商」，取页头那个。
  await user.click(screen.getAllByRole('button', { name: '添加供应商' })[0]!);
  const dialog = screen.getByRole('dialog');
  await user.type(within(dialog).getByLabelText('供应商名称'), '新服务');
  await user.type(within(dialog).getByLabelText('Base URL'), 'https://api.example.com/v1');
  await user.type(within(dialog).getByLabelText('API Key'), 'synthetic-secret');
  await user.click(within(dialog).getByRole('button', { name: '保存' }));

  // 备注留空时记作「默认」，不需要人为了一个内部标签再想一个名字。
  await waitFor(() => expect(addCredential).toHaveBeenCalledWith('p_new', '默认', 'synthetic-secret'));
  expect(selectCredential).toHaveBeenCalledWith('p_new', 'k_new');
  // 弹窗不关：就地变成这家供应商的编辑态，接着还能获取可用模型、加模型。
  expect(await screen.findByText('已保存。')).toBeInTheDocument();
  expect(within(dialog).getByRole('button', { name: '获取可用模型' })).toBeInTheDocument();
  expect(within(dialog).getByRole('button', { name: '添加模型' })).toBeInTheDocument();
});

test('没有 Key 时点获取可用模型会说明缺什么，而不是静默失败', async () => {
  const user = userEvent.setup();
  const credential = { id: 'k_1', providerId: 'p_test', label: '备用', secretRef: 'r', secretVersion: 1,
    maskedSuffix: '••••1c7', status: 'saved' as const, scope: null, lastVerifiedAt: null, version: 1,
    createdAt: '2026-09-18T00:00:00Z' };
  const client = testClient({ listProviders: vi.fn().mockResolvedValue({ items: [provider], nextCursor: null }),
    listCredentials: vi.fn().mockResolvedValue([credential]) });
  render(<App client={client} />);
  await screen.findByText('测试供应商');
  await user.click(within(screen.getByRole('navigation')).getByRole('button', { name: '供应商与模型' }));
  await user.click(screen.getByRole('button', { name: '编辑配置' }));
  const dialog = await screen.findByRole('dialog');

  // Key 有，但不是当前 Key：先把原因说出来，别让人对着一个没反应的按钮猜。
  await user.click(within(dialog).getByRole('button', { name: '获取可用模型' }));
  expect(await screen.findByRole('alert')).toHaveTextContent('先给这家供应商添加并选择一个 Key');
  expect(client.discoverModels).not.toHaveBeenCalled();
});

test('新建时点获取可用模型：先把地址与 Key 存下来，再去问上游', async () => {
  const user = userEvent.setup();
  const saveProvider = vi.fn().mockResolvedValue({ ...provider, id: 'p_new', name: '新服务', activeCredentialId: 'k_new' });
  const addCredential = vi.fn().mockResolvedValue({ id: 'k_new', label: '默认' });
  const discoverModels = vi.fn().mockResolvedValue([{ upstreamId: 'vendor/a', displayName: 'A', alreadySaved: false }]);
  const selectCredential = vi.fn().mockResolvedValue(undefined);
  const client = testClient({ saveProvider, addCredential, selectCredential,
    discoverModels, listCredentials: vi.fn().mockResolvedValue([]) });
  render(<App client={client} />);
  await user.click(within(screen.getByRole('navigation')).getByRole('button', { name: '供应商与模型' }));
  await user.click(screen.getAllByRole('button', { name: '添加供应商' })[0]!);
  const dialog = screen.getByRole('dialog');
  await user.type(within(dialog).getByLabelText('供应商名称'), '新服务');
  await user.type(within(dialog).getByLabelText('Base URL'), 'https://api.example.com/v1');
  await user.type(within(dialog).getByLabelText('API Key'), 'synthetic-secret');

  // 地址与 Key 是这次请求的前提：点「获取可用模型」时先落库，不再让人先点一次保存。
  await user.click(within(dialog).getByRole('button', { name: '获取可用模型' }));

  await waitFor(() => expect(discoverModels).toHaveBeenCalledWith('p_new', 'k_new'));
  expect(saveProvider).toHaveBeenCalledWith(expect.objectContaining({ endpoint: 'https://api.example.com/v1' }), 0);
  expect(addCredential).toHaveBeenCalledWith('p_new', '默认', 'synthetic-secret');
  expect(selectCredential).toHaveBeenCalledWith('p_new', 'k_new');
  expect(await screen.findByRole('dialog', { name: '选择要添加的模型' })).toBeInTheDocument();
});

test('获取可用模型：弹窗里勾选确认，一次把模型按默认长度加进来', async () => {
  const user = userEvent.setup();
  const active = { ...provider, activeCredentialId: 'k_1' };
  const credential = { id: 'k_1', providerId: 'p_test', label: '日常', secretRef: 'r', secretVersion: 1,
    maskedSuffix: '••••1c7', status: 'saved' as const, scope: null, lastVerifiedAt: null, version: 1,
    createdAt: '2026-09-18T00:00:00Z' };
  const saveModel = vi.fn().mockResolvedValue({});
  const client = testClient({ listProviders: vi.fn().mockResolvedValue({ items: [active], nextCursor: null }),
    listCredentials: vi.fn().mockResolvedValue([credential]), saveModel,
    discoverModels: vi.fn().mockResolvedValue([
      { upstreamId: 'vendor/old', displayName: '已添加的模型', alreadySaved: true },
      { upstreamId: 'vendor/deepseek-v4.1', displayName: 'deepseek-v4.1', alreadySaved: false },
      { upstreamId: 'vendor/vision', displayName: '视觉模型', alreadySaved: false },
    ]) });
  render(<App client={client} />);
  await screen.findByText('测试供应商');
  await user.click(within(screen.getByRole('navigation')).getByRole('button', { name: '供应商与模型' }));
  await user.click(screen.getByRole('button', { name: '编辑配置' }));
  const dialog = await screen.findByRole('dialog');
  await user.click(within(dialog).getByRole('button', { name: '获取可用模型' }));

  const picker = await screen.findByRole('dialog', { name: '选择要添加的模型' });
  // 已经在库里的那条不能再加一次，也不再默认勾选。
  expect(within(picker).getByRole('checkbox', { name: /vendor\/old/ })).toBeDisabled();
  expect(within(picker).getByRole('checkbox', { name: /vendor\/deepseek-v4\.1/ })).toBeChecked();
  // 上游不返回窗口大小：默认值写在底栏，加之前就能看到。
  expect(within(picker).getByText(/128,000 \/ 8,192 的默认值/)).toBeInTheDocument();

  await user.click(within(picker).getByRole('button', { name: '添加 2 个模型' }));

  await waitFor(() => expect(saveModel).toHaveBeenCalledTimes(2));
  expect(saveModel).toHaveBeenCalledWith(expect.objectContaining({
    providerId: 'p_test', upstreamId: 'vendor/deepseek-v4.1', displayName: 'deepseek-v4.1', inCatalog: true,
  }), 0);
  const policy = saveModel.mock.calls[0]![0]!.policy;
  expect(policy.contextLimit).toBe(128_000);
  expect(policy.outputLimit).toBe(8_192);
  expect(screen.getByText('已添加 2 个模型。')).toBeInTheDocument();
});

test('手工添加模型：模型 ID 与长度落进策略，供应商弹窗留在原地', async () => {
  const user = userEvent.setup();
  const active = { ...provider, activeCredentialId: 'k_1' };
  const credential = { id: 'k_1', providerId: 'p_test', label: '日常', secretRef: 'r', secretVersion: 1,
    maskedSuffix: '••••1c7', status: 'saved' as const, scope: null, lastVerifiedAt: null, version: 1,
    createdAt: '2026-09-18T00:00:00Z' };
  const saveModel = vi.fn().mockResolvedValue({});
  const client = testClient({ listProviders: vi.fn().mockResolvedValue({ items: [active], nextCursor: null }),
    listCredentials: vi.fn().mockResolvedValue([credential]), saveModel });
  render(<App client={client} />);
  await screen.findByText('测试供应商');
  await user.click(within(screen.getByRole('navigation')).getByRole('button', { name: '供应商与模型' }));
  await user.click(screen.getByRole('button', { name: '编辑配置' }));
  const dialog = await screen.findByRole('dialog');
  await user.click(within(dialog).getByRole('button', { name: '添加模型' }));

  // 模型表单叠在供应商弹窗上：DOM 里两层都在，说明供应商弹窗没有关。
  expect(document.querySelectorAll('[role="dialog"]').length).toBe(2);
  const form = screen.getByRole('dialog');
  await user.type(within(form).getByLabelText('模型 ID'), 'vendor/manual');
  await user.type(within(form).getByLabelText('上下文窗口'), '128k');
  await user.click(within(form).getByRole('button', { name: '保存' }));

  // 显示名沿用模型 ID；128k 被解析成数值，最大输出用智能配置的默认值补齐。
  expect(saveModel).toHaveBeenCalledWith(expect.objectContaining({
    providerId: 'p_test', upstreamId: 'vendor/manual', displayName: 'vendor/manual', inCatalog: true,
  }), 0);
  expect(saveModel.mock.calls[0]![0]!.policy.contextLimit).toBe(128_000);
  expect(saveModel.mock.calls[0]![0]!.policy.outputLimit).toBe(8_192);
});

/** 供应商弹窗里的一行模型：徽章、四个动作，以及它们各自的结论。 */
function modelRow() {
  return { id: 'm_1', providerId: 'p_test', upstreamId: 'vendor/a', catalogAlias: 'gs/m_1',
    displayName: '目录中的模型', lifecycle: 'saved' as const, hostState: 'pending_apply' as const, inCatalog: true,
    policy: { contextLimit: 1_048_576, outputLimit: 8_192, compactLimit: null,
      reasoning: { support: 'unknown' as const, control: 'none' as const, allowedValues: [], defaultValue: null, budgetTokens: null, mappingId: null },
      inputs: [], tools: { functionTools: 'unknown' as const, parallelTools: 'unknown' as const, customTools: 'unknown' as const, verification: 'declared' as const } },
    displayNameLayer: { discovered: null, userValue: null, overridden: false }, capabilityRevision: 1, version: 3,
    createdAt: '2026-09-18T00:00:00Z', updatedAt: '2026-09-18T00:00:00Z' };
}

test('供应商弹窗就地显示这家供应商的模型与长度徽章', async () => {
  const user = userEvent.setup();
  const active = { ...provider, activeCredentialId: 'k_1' };
  const client = testClient({ listProviders: vi.fn().mockResolvedValue({ items: [active], nextCursor: null }),
    listCredentials: vi.fn().mockResolvedValue([]), listModels: vi.fn().mockResolvedValue([modelRow()]) });
  render(<App client={client} />);
  await screen.findByText('测试供应商');
  await user.click(within(screen.getByRole('navigation')).getByRole('button', { name: '供应商与模型' }));
  await user.click(screen.getByRole('button', { name: '编辑配置' }));
  const dialog = await screen.findByRole('dialog');

  expect(await within(dialog).findByText('目录中的模型')).toBeInTheDocument();
  // 行上直接给长度：1048576 读成 1M，不用自己数零。
  expect(within(dialog).getByTitle('上下文窗口 1,048,576 Token')).toHaveTextContent('1M');
  // 三个动作 + 一个目录开关都在行内，不用回到详情页找入口。
  expect(within(dialog).getByRole('button', { name: '测试 目录中的模型 的连接' })).toBeInTheDocument();
  expect(within(dialog).getByRole('button', { name: '编辑 目录中的模型' })).toBeInTheDocument();
  expect(within(dialog).getByRole('button', { name: '删除模型 目录中的模型' })).toBeInTheDocument();
  expect(within(dialog).getByRole('switch', { name: '把 目录中的模型 放进 Codex 目录' })).toBeChecked();
});

test('模型行的测试连接把结论说出来，成功与失败都不含糊', async () => {
  const user = userEvent.setup();
  const active = { ...provider, activeCredentialId: 'k_1' };
  const credential = { id: 'k_1', providerId: 'p_test', label: '日常', secretRef: 'r', secretVersion: 1,
    maskedSuffix: '••••1c7', status: 'saved' as const, scope: null, lastVerifiedAt: null, version: 1,
    createdAt: '2026-09-18T00:00:00Z' };
  const startProbe = vi.fn().mockResolvedValue({ id: 'probe_1', targetLabel: 'x', startedAt: '2026-09-18T00:00:00Z',
    stages: [{ stageKey: 'connect', status: 'passed', messageKey: 'probe.connected' }] });
  const client = testClient({ listProviders: vi.fn().mockResolvedValue({ items: [active], nextCursor: null }),
    listCredentials: vi.fn().mockResolvedValue([credential]), listModels: vi.fn().mockResolvedValue([modelRow()]), startProbe });
  render(<App client={client} />);
  await screen.findByText('测试供应商');
  await user.click(within(screen.getByRole('navigation')).getByRole('button', { name: '供应商与模型' }));
  await user.click(screen.getByRole('button', { name: '编辑配置' }));
  const dialog = await screen.findByRole('dialog');
  await user.click(await within(dialog).findByRole('button', { name: '测试 目录中的模型 的连接' }));

  // 结论指名道姓：哪家供应商、哪个模型。只读探测，不发真实生成。
  expect(await screen.findByText('测试供应商 / 目录中的模型 连接成功')).toBeInTheDocument();
  expect(startProbe).toHaveBeenCalledWith({ providerId: 'p_test', modelId: 'm_1', credentialId: 'k_1' }, { includeGenerate: false });

  startProbe.mockResolvedValue({ id: 'probe_2', targetLabel: 'x', startedAt: '2026-09-18T00:00:00Z',
    stages: [{ stageKey: 'credential', status: 'failed', messageKey: 'probe.credentialRejected' }] });
  await user.click(within(dialog).getByRole('button', { name: '测试 目录中的模型 的连接' }));
  expect(await screen.findByText(/连接失败/)).toBeInTheDocument();
});

test('删除已纳入目录的模型：先移出目录，再用新版本号删除', async () => {
  const user = userEvent.setup();
  const saveModel = vi.fn().mockResolvedValue({ ...modelRow(), inCatalog: false, version: 4 });
  const deleteModel = vi.fn().mockResolvedValue(undefined);
  const client = testClient({ listProviders: vi.fn().mockResolvedValue({ items: [provider], nextCursor: null }),
    listModels: vi.fn().mockResolvedValue([modelRow()]), saveModel, deleteModel });
  render(<App client={client} />);
  await screen.findByText('测试供应商');
  await user.click(within(screen.getByRole('navigation')).getByRole('button', { name: '供应商与模型' }));
  await user.click(screen.getByRole('button', { name: '编辑配置' }));
  const dialog = await screen.findByRole('dialog');
  await user.click(await within(dialog).findByRole('button', { name: '删除模型 目录中的模型' }));

  const confirm = screen.getAllByRole('dialog').at(-1)!;
  expect(within(confirm).getByText(/先移出目录/)).toBeInTheDocument();
  expect(deleteModel).not.toHaveBeenCalled();

  await user.click(within(confirm).getByRole('button', { name: '删除模型' }));
  await waitFor(() => expect(deleteModel).toHaveBeenCalledWith('m_1', 4));
  expect(saveModel).toHaveBeenCalledWith(expect.objectContaining({ id: 'm_1', inCatalog: false }), 3);
});

test('编辑器有未保存修改时侧栏导航先确认，放弃后才真正离开', async () => {
  const user = userEvent.setup();
  const model = { id: 'm_1', providerId: 'p_test', upstreamId: 'vendor/a', catalogAlias: 'gs/m_1',
    displayName: '目录中的模型', lifecycle: 'saved' as const, hostState: 'pending_apply' as const, inCatalog: true,
    policy: { contextLimit: 128_000, outputLimit: 8_192, compactLimit: null,
      reasoning: { support: 'unknown' as const, control: 'none' as const, allowedValues: [], defaultValue: null, budgetTokens: null, mappingId: null },
      inputs: [], tools: { functionTools: 'unknown' as const, parallelTools: 'unknown' as const, customTools: 'unknown' as const, verification: 'declared' as const } },
    displayNameLayer: { discovered: null, userValue: null, overridden: false }, capabilityRevision: 1, version: 3,
    createdAt: '2026-09-18T00:00:00Z', updatedAt: '2026-09-18T00:00:00Z' };
  const client = testClient({ listProviders: vi.fn().mockResolvedValue({ items: [provider], nextCursor: null }),
    listModels: vi.fn().mockResolvedValue([model]) });
  render(<App client={client} />);
  await screen.findByText('测试供应商');
  await user.click(within(screen.getByRole('navigation')).getByRole('button', { name: '供应商与模型' }));
  await user.click(await screen.findByRole('button', { name: '编辑 目录中的模型' }));
  expect(await screen.findByRole('heading', { level: 1, name: '目录中的模型' })).toBeInTheDocument();

  await user.type(screen.getByLabelText('显示名称'), '改一下');
  await user.click(within(screen.getByRole('navigation')).getByRole('button', { name: '概览' }));

  // 界面不再说谎：确认框弹出时正文仍停在编辑器上（确认框是模态，
  // 底下的 h1 被标 aria-hidden，所以从 DOM 直接断言）。
  expect(document.querySelector('h1')).toHaveTextContent('目录中的模型');
  expect(screen.getByRole('dialog')).toBeInTheDocument();
  await user.click(screen.getByRole('button', { name: '放弃并离开' }));
  expect(await screen.findByRole('heading', { level: 1, name: '概览' })).toBeInTheDocument();
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
  await user.click(within(screen.getByRole('navigation')).getByRole('button', { name: '供应商与模型' }));
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
  await user.click(within(screen.getByRole('navigation')).getByRole('button', { name: '供应商与模型' }));
  await user.click(await screen.findByRole('button', { name: '更多操作 目录中的模型' }));
  await user.click(screen.getByRole('menuitem', { name: '移出 Codex 目录' }));
  const dialog = await screen.findByRole('dialog');
  expect(within(dialog).getByText(/需要重新生成差异并应用/)).toBeInTheDocument();
  expect(saveModel).not.toHaveBeenCalled();

  await user.click(within(dialog).getByRole('button', { name: '移出目录' }));
  expect(saveModel).toHaveBeenCalledWith(expect.objectContaining({ id: 'm_1', inCatalog: false }), 3);
});

/**
 * 模拟全新安装。向导的「已看过」标记存在 localStorage 里，且用例之间刻意不清
 * （见 vitest.setup.ts），所以「首次接入」这类用例必须自己清掉，否则会静默地
 * 跑在「已经看过向导」的状态下，断言一个永远不会出现的向导。
 */
function freshInstall() { localStorage.removeItem('gptswitch.onboarding.dismissed'); }

test('回归：保存第一个供应商之后向导不消失，第 3 步仍然走得到', async () => {
  /*
   * 向导曾经是从「零供应商」推导出来的：保存第一家供应商的瞬间它就整体消失，
   * 第 3 步「测试并应用」永远走不到，用户被扔在一个只走了一半的流程里。
   * 向导一旦打开就该留到用户自己结束它。
   */
  freshInstall();
  const user = userEvent.setup();
  let saved = false;
  const stored: Provider = { ...provider, id: 'p_test', activeCredentialId: 'k_1', version: 2 };
  const key: Credential = { id: 'k_1', providerId: 'p_test', label: '默认', secretRef: 'gptswitch/p_test/k_1/v1',
    secretVersion: 1, maskedSuffix: '••••1234', status: 'verified', scope: null, lastVerifiedAt: null,
    version: 1, createdAt: '2026-09-18T00:00:00Z' };
  const client = testClient({
    listProviders: vi.fn().mockImplementation(async () => ({ items: saved ? [stored] : [], nextCursor: null })),
    listCredentials: vi.fn().mockImplementation(async () => saved ? [key] : []),
    saveProvider: vi.fn().mockImplementation(async () => { saved = true; return stored; }),
    addCredential: vi.fn().mockResolvedValue(key),
    selectCredential: vi.fn().mockResolvedValue(undefined),
  });
  render(<App client={client} />);

  // 首次接入：没有供应商，向导自动展开。
  expect(await screen.findByRole('heading', { name: '接入向导' })).toBeInTheDocument();

  await user.click(screen.getByRole('button', { name: '下一步' }));
  await user.click(await screen.findByRole('button', { name: '添加供应商' }));
  const dialog = await screen.findByRole('dialog');
  await user.type(within(dialog).getByLabelText('供应商名称'), '测试服务');
  await user.type(within(dialog).getByLabelText('Base URL'), 'https://example.test/v1');
  await user.type(within(dialog).getByLabelText('API Key'), 'synthetic-secret');
  await user.click(within(dialog).getByRole('button', { name: '保存' }));
  await waitFor(() => expect(client.saveProvider).toHaveBeenCalled());
  await user.click(within(dialog).getByRole('button', { name: '取消' }));

  // 供应商已经存在了，但向导必须还在——否则第 3 步永远到不了。
  expect(await screen.findByRole('heading', { name: '接入向导' })).toBeInTheDocument();
  // 清单要反映刚存进去的那家供应商，而不是还停在空白状态。
  expect(await screen.findByText('已添加 1 个供应商')).toBeInTheDocument();
  await user.click(screen.getByRole('button', { name: '下一步' }));
  expect(screen.getByRole('heading', { name: '测试并应用' })).toBeInTheDocument();
  // 第 3 步要自己说清为什么「应用」会重启 Codex，而不是让用户自己猜。
  expect(screen.getByText(/Codex 只在启动时读配置/)).toBeInTheDocument();
});

test('退出向导后可以从设置页重新打开', async () => {
  freshInstall();
  const user = userEvent.setup();
  render(<App client={testClient()} />);
  await user.click(await screen.findByRole('button', { name: '稍后再说' }));
  expect(screen.queryByRole('heading', { name: '接入向导' })).not.toBeInTheDocument();

  await user.click(within(screen.getByRole('navigation')).getByRole('button', { name: '供应商与模型' }));
  await user.click(screen.getByRole('button', { name: '设置' }));
  await user.click(await screen.findByRole('button', { name: '重新打开接入向导' }));
  expect(await screen.findByRole('heading', { name: '接入向导' })).toBeInTheDocument();
});

test('Key 池：加第二个 Key、改名、停用、删除都能做，当前 Key 受保护', async () => {
  // 回归：核心与数据库一直支持多个 Key，但界面只有一个输入框——留空＝不动、填了＝替换当前那个。
  // 于是「新增、替换、禁用」这三件事里有三件做不了，而它们是 P0 要求。
  const user = userEvent.setup();
  const first: Credential = { id: 'k_1', providerId: 'p_test', label: '日常', secretRef: 'r1', secretVersion: 1,
    maskedSuffix: '••••4f2a', status: 'verified', scope: null, lastVerifiedAt: null, version: 1, createdAt: '2026-09-18T00:00:00Z' };
  const second: Credential = { ...first, id: 'k_2', label: '备用', secretRef: 'r2', maskedSuffix: '••••91c7', status: 'saved', version: 1 };
  const active = { ...provider, activeCredentialId: 'k_1' };
  const addCredential = vi.fn().mockResolvedValue(second);
  const renameCredential = vi.fn().mockResolvedValue({ ...second, label: '按量', version: 2 });
  const setCredentialDisabled = vi.fn().mockResolvedValue({ ...second, status: 'disabled', version: 2 });
  const deleteCredential = vi.fn().mockResolvedValue(undefined);
  const client = testClient({
    listProviders: vi.fn().mockResolvedValue({ items: [active], nextCursor: null }),
    listCredentials: vi.fn().mockResolvedValue([first, second]),
    addCredential, renameCredential, setCredentialDisabled, deleteCredential,
  });
  render(<App client={client} />);
  await screen.findByText('测试供应商');
  await user.click(within(screen.getByRole('navigation')).getByRole('button', { name: '供应商与模型' }));
  await user.click(screen.getByRole('button', { name: '编辑配置' }));
  const dialog = await screen.findByRole('dialog');

  const pool = within(dialog).getByRole('region', { name: '已保存的 Key' });
  expect(within(pool).getByText('日常')).toBeInTheDocument();
  expect(within(pool).getByText('备用')).toBeInTheDocument();

  // 当前 Key 不能停用也不能删——理由写在 title 里，而不是点下去才报错。
  const currentRow = within(pool).getAllByRole('listitem')[0]!;
  expect(within(currentRow).getByRole('button', { name: '停用' })).toBeDisabled();
  expect(within(currentRow).getByRole('button', { name: '停用' })).toHaveAttribute('title', '当前 Key 不能停用，请先把别的 Key 设为当前');

  // 非当前的那条：停用会走核心调用，并把版本号带上。
  const otherRow = within(pool).getAllByRole('listitem')[1]!;
  await user.click(within(otherRow).getByRole('button', { name: '停用' }));
  await waitFor(() => expect(setCredentialDisabled).toHaveBeenCalledWith('k_2', true, 1));

  // 改名：行内编辑，保存时带上版本号。
  await user.click(within(otherRow).getByRole('button', { name: '编辑' }));
  const nameInput = within(pool).getByLabelText('重命名「备用」');
  await user.clear(nameInput);
  await user.type(nameInput, '按量');
  await user.click(within(pool).getByRole('button', { name: '保存' }));
  await waitFor(() => expect(renameCredential).toHaveBeenCalledWith('k_2', '按量', 1));

  // 加第二个 Key：行内表单，备注名必填。
  await user.click(within(pool).getByRole('button', { name: '添加 Key' }));
  await user.click(within(pool).getByRole('button', { name: '保存这个 Key' }));
  expect(await screen.findByRole('alert')).toHaveTextContent('备注名');
  expect(addCredential).not.toHaveBeenCalled();
  await user.type(within(pool).getByLabelText('备注名'), '第三个');
  await user.type(within(pool).getByLabelText('API Key'), 'synthetic-secret');
  await user.click(within(pool).getByRole('button', { name: '保存这个 Key' }));
  await waitFor(() => expect(addCredential).toHaveBeenCalledWith('p_test', '第三个', 'synthetic-secret'));

  // 删除非当前 Key。
  await user.click(within(otherRow).getByRole('button', { name: '删除' }));
  await waitFor(() => expect(deleteCredential).toHaveBeenCalledWith('k_2'));
});

test('供应商列表可以搜索：过滤逻辑早就在，缺的是输入框', async () => {
  // 回归：`query` 状态、`visibleProviders` 过滤与「没有匹配」的空态都已经写好，
  // 但没有任何地方能输入——供应商一多只能在 300px 宽的列表里用眼睛找。
  const user = userEvent.setup();
  const other = { ...provider, id: 'p_other', name: '另一家' };
  render(<App client={testClient({ listProviders: vi.fn().mockResolvedValue({ items: [provider, other], nextCursor: null }) })} />);
  await user.click(within(screen.getByRole('navigation')).getByRole('button', { name: '供应商与模型' }));
  const list = await screen.findByRole('region', { name: '供应商列表' });
  expect(within(list).getByText('测试供应商')).toBeInTheDocument();
  expect(within(list).getByText('另一家')).toBeInTheDocument();

  await user.type(screen.getByLabelText('搜索供应商'), '另一');
  expect(within(list).queryByText('测试供应商')).not.toBeInTheDocument();
  expect(within(list).getByText('另一家')).toBeInTheDocument();

  // 搜不到时给空态，而不是一张空白列表。
  await user.clear(screen.getByLabelText('搜索供应商'));
  await user.type(screen.getByLabelText('搜索供应商'), '不存在的东西');
  expect(await screen.findByText('没有匹配的供应商')).toBeInTheDocument();
});
