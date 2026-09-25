import { render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import type { ApplyStage, FieldChange, OperationEvent } from '@/contracts/types';
import { App } from '@/app/App';
import { coexistOff, instance, plan, testClient } from '../../../tests/helpers/client';

/** 提示宿主：页面里也有别的 role=status（等待重载、检测状态），断言提示时必须限定范围。 */
const notifications = () => screen.getByLabelText('通知');

function event(phase: ApplyStage): OperationEvent {
  return { schemaVersion: 1, operationId: 'op_1', sequence: 1, phase, revisionId: 'rev_test',
    messageKey: `stage.${phase}`, safeArgs: {}, cancellable: false, timestamp: '2026-09-18T00:00:00Z' };
}

const modelChange: FieldChange = { keyPath: 'model', before: null, after: 'p_test-alias', reasonKey: 'reason.defaultModel' };
const providerChange: FieldChange = { keyPath: 'model_providers.gptswitch', before: null, after: 'base_url = …', reasonKey: 'reason.gatewayProvider' };

async function openCodexPage(client: Parameters<typeof testClient>[0]) {
  const user = userEvent.setup();
  render(<App client={testClient(client)} />);
  await user.click(within(screen.getByRole('navigation')).getByRole('button', { name: '配置' }));
  await screen.findByText('Codex 实例');
  return user;
}

/** 未检测到实例时页面不渲染实例卡片，需要单独进入。 */
async function openEmptyCodexPage() {
  const user = userEvent.setup();
  render(<App client={testClient({ detectInstances: vi.fn().mockResolvedValue([]) })} />);
  await user.click(within(screen.getByRole('navigation')).getByRole('button', { name: '配置' }));
  await screen.findByText('未检测到 Codex');
  return user;
}

test('提交成功只显示等待 Codex 重新加载，绝不自行宣称已加载', async () => {
  const confirmReload = vi.fn();
  const restartHost = vi.fn().mockResolvedValue({ appPath: '/Applications/ChatGPT.app', quitConfirmed: true, quitForced: false, launchedConfirmed: true });
  const user = await openCodexPage({
    detectInstances: vi.fn().mockResolvedValue([instance]),
    planApply: vi.fn().mockResolvedValue(plan([modelChange, providerChange])),
    executeApply: vi.fn().mockResolvedValue({ operationId: 'op_1' }),
    applyStatus: vi.fn().mockResolvedValue({ operationId: 'op_1', open: true, events: [event('prepared'), event('awaiting_reload')] }),
    confirmReload,
    restartHost,
  });

  await user.click(screen.getByRole('button', { name: '应用到 Codex' }));
  // 主按钮由核心的 reloadScope 决定文案，不是“应用”。
  await user.click(screen.getByRole('button', { name: '应用并重新加载' }));

  expect(await screen.findByText(/请在 Codex 中重新加载或重开窗口后再确认/)).toBeInTheDocument();
  // 只有用户确认后才允许出现成功文案。
  expect(screen.queryByText('Codex 已重新加载并核验本次目录。')).not.toBeInTheDocument();
  expect(confirmReload).not.toHaveBeenCalled();
  // 配置写完就自动重启宿主一次，用户不必再点「重启 Codex」；
  // 但文案仍然只是「已提交 + 已重启」，不说它已经加载了新目录。
  expect(restartHost).toHaveBeenCalledWith(instance.id);
  // 宿主里可能同时存在「已生成差异」那条信息提示，所以按文案断言而不是按角色取唯一一条。
  expect(within(notifications()).getByText(/配置已提交，Codex 已重启/)).toBeInTheDocument();
});

test('回归：还原之后同样重启 Codex，并说清它回到了原生', async () => {
  // 真机上的表现：点了还原、也重启了 Codex，左下角却仍然显示本工具——一是记录里的基线
  // 本身就是我们写的（核心侧已修），二是还原这条路过去**不重启宿主**，界面里看不出变化。
  const restartHost = vi.fn().mockResolvedValue({ appPath: '/Applications/ChatGPT.app', quitConfirmed: true, quitForced: false, launchedConfirmed: true });
  const user = await openCodexPage({
    detectInstances: vi.fn().mockResolvedValue([instance]),
    planRestore: vi.fn().mockResolvedValue(plan([modelChange], { changes: [{ ...modelChange, reasonKey: 'reason.restore' }] })),
    executeRestore: vi.fn().mockResolvedValue({ operationId: 'op_r' }),
    applyStatus: vi.fn().mockResolvedValue({ operationId: 'op_r', open: false, events: [event('restored')] }),
    restartHost,
  });

  await user.click(screen.getByRole('button', { name: '还原为原生 Codex' }));
  await user.click(await screen.findByRole('button', { name: '确认还原' }));

  expect(restartHost).toHaveBeenCalledWith(instance.id);
  expect(await within(notifications()).findByText(/回到原生登录与原生模型列表/)).toBeInTheDocument();
});

test('还原后宿主没能重启时，明说需要手动打开', async () => {
  const restartHost = vi.fn().mockResolvedValue({ appPath: '/Applications/ChatGPT.app', quitConfirmed: true, quitForced: false, launchedConfirmed: false });
  const user = await openCodexPage({
    detectInstances: vi.fn().mockResolvedValue([instance]),
    planRestore: vi.fn().mockResolvedValue(plan([modelChange], { changes: [{ ...modelChange, reasonKey: 'reason.restore' }] })),
    executeRestore: vi.fn().mockResolvedValue({ operationId: 'op_r' }),
    applyStatus: vi.fn().mockResolvedValue({ operationId: 'op_r', open: false, events: [event('restored')] }),
    restartHost,
  });

  await user.click(screen.getByRole('button', { name: '还原为原生 Codex' }));
  await user.click(await screen.findByRole('button', { name: '确认还原' }));

  const alert = await within(notifications()).findByRole('alert');
  expect(alert).toHaveTextContent(/没能重启/);
});

test('用户确认宿主已重新加载后才进入已核验', async () => {
  const confirmReload = vi.fn().mockResolvedValue({ operationId: 'op_1', open: false, events: [event('awaiting_reload'), event('verified')] });
  const user = await openCodexPage({
    detectInstances: vi.fn().mockResolvedValue([instance]),
    planApply: vi.fn().mockResolvedValue(plan([modelChange])),
    executeApply: vi.fn().mockResolvedValue({ operationId: 'op_1' }),
    applyStatus: vi.fn().mockResolvedValue({ operationId: 'op_1', open: true, events: [event('awaiting_reload')] }),
    confirmReload,
  });

  await user.click(screen.getByRole('button', { name: '应用到 Codex' }));
  await user.click(screen.getByRole('button', { name: '应用并重新加载' }));
  await user.click(await screen.findByRole('button', { name: 'Codex 已重新加载' }));

  expect(confirmReload).toHaveBeenCalledWith('op_1', true);
  expect(await screen.findByText('Codex 已重新加载并核验本次目录。')).toBeInTheDocument();
});

/// 计划带的是生成时那份配置的哈希，而 Codex 自己也会写 config.toml（启动时补项目记录），
/// 所以过期是常态：必须自动重生成再提交，而不是让用户去理解 CAS、更不是让他自己找「重新比较」。
test('回归：计划过期（Codex 自己改过配置）时自动重生成再提交', async () => {
  const planApply = vi.fn()
    .mockResolvedValueOnce(plan([modelChange]))
    .mockResolvedValueOnce(plan([modelChange], { id: 'plan_fresh' }));
  const executeApply = vi.fn()
    .mockRejectedValueOnce({ code: 'CONFIG_CHANGED', messageKey: 'error.configChanged',
      safeDetails: ['配置文件在计划生成后被其他程序修改'], retryable: false, recoveryActions: [] })
    .mockResolvedValueOnce({ operationId: 'op_1' });
  const restartHost = vi.fn().mockResolvedValue({ appPath: '/Applications/ChatGPT.app', quitConfirmed: true, quitForced: false, launchedConfirmed: true });
  const user = await openCodexPage({
    detectInstances: vi.fn().mockResolvedValue([instance]),
    planApply, executeApply, restartHost,
    applyStatus: vi.fn().mockResolvedValue({ operationId: 'op_1', open: false, events: [event('verified')] }),
  });

  await user.click(screen.getByRole('button', { name: '应用到 Codex' }));
  await user.click(screen.getByRole('button', { name: '应用并重新加载' }));

  expect(planApply).toHaveBeenCalledTimes(2);
  expect(executeApply).toHaveBeenLastCalledWith(expect.objectContaining({ planId: 'plan_fresh' }));
  expect(restartHost).toHaveBeenCalled();
  expect(await within(notifications()).findByText(/Codex 自己改过配置文件/)).toBeInTheDocument();
});

test('重生成之后仍然失败：把原因留在差异弹窗里，不谎称已提交', async () => {
  const planApply = vi.fn()
    .mockResolvedValueOnce(plan([modelChange]))
    .mockResolvedValueOnce(plan([modelChange], { id: 'plan_fresh' }));
  const user = await openCodexPage({
    detectInstances: vi.fn().mockResolvedValue([instance]),
    planApply,
    executeApply: vi.fn().mockRejectedValue({ code: 'CONFIG_CHANGED', messageKey: 'error.configChanged',
      safeDetails: ['配置文件在计划生成后被其他程序修改'], retryable: false, recoveryActions: [] }),
    applyStatus: vi.fn().mockResolvedValue({ operationId: 'op_1', open: false, events: [event('conflict')] }),
  });

  await user.click(screen.getByRole('button', { name: '应用到 Codex' }));
  await user.click(screen.getByRole('button', { name: '应用并重新加载' }));

  // 自动重生成一次（共两次计划），第二次仍被拒 → 原因留在弹窗里，弹窗不关。
  expect(planApply).toHaveBeenCalledTimes(2);
  expect(await within(screen.getByRole('dialog')).findByRole('alert')).toHaveTextContent('配置文件在计划生成后被其他程序修改');
  expect(screen.queryByText('Codex 已重新加载并核验本次目录。')).not.toBeInTheDocument();
});

test('差异按原因分组展示，不把内部 reasonKey 泄露到界面', async () => {
  const user = await openCodexPage({
    detectInstances: vi.fn().mockResolvedValue([instance]),
    planApply: vi.fn().mockResolvedValue(plan([modelChange, providerChange, { keyPath: 'unknown_field', before: 'a', after: 'b', reasonKey: 'reason.to' }])),
  });

  await user.click(screen.getByRole('button', { name: '应用到 Codex' }));

  expect(await screen.findByText('供应商路由')).toBeInTheDocument();
  expect(screen.getByText('模型目录')).toBeInTheDocument();
  expect(screen.getByText('其他字段')).toBeInTheDocument();
  expect(screen.getByText('应用后作为默认模型')).toBeInTheDocument();
  expect(screen.queryByText('reason.to')).not.toBeInTheDocument();
  expect(screen.getByText('受管字段变更')).toBeInTheDocument();
});

test('编译警告显示可读文案，不把内部 messageKey 摆给用户', async () => {
  const user = await openCodexPage({
    detectInstances: vi.fn().mockResolvedValue([instance]),
    planApply: vi.fn().mockResolvedValue(plan([modelChange], {
      warnings: ['warning.reasoningNotSelectableInHost：模型甲的推理控制在 Codex 中不可切换', '完全未知的警告正文'],
    })),
  });

  await user.click(screen.getByRole('button', { name: '应用到 Codex' }));

  expect(await screen.findByText('思考档位在 Codex 中不可切换')).toBeInTheDocument();
  expect(screen.getByText(/模型甲的推理控制在 Codex 中不可切换/)).toBeInTheDocument();
  expect(screen.queryByText(/warning\.reasoningNotSelectableInHost/)).not.toBeInTheDocument();
  // 未知 key 走通用标签，正文仍然保留。
  expect(screen.getByText('目录提示')).toBeInTheDocument();
  expect(screen.getByText(/完全未知的警告正文/)).toBeInTheDocument();
});



test('重启宿主：先确认，再调用一次，并按确认到的结果说话', async () => {
  const user = userEvent.setup();
  const restartHost = vi.fn().mockResolvedValue({ appPath: '/Applications/ChatGPT.app', quitConfirmed: true, quitForced: false, launchedConfirmed: true });
  render(<App client={testClient({ detectInstances: vi.fn().mockResolvedValue([instance]), restartHost })} />);
  await user.click(within(screen.getByRole('navigation')).getByRole('button', { name: '配置' }));
  await screen.findByText('Codex 实例');

  await user.click(screen.getByRole('button', { name: '重启 Codex' }));
  const dialog = await screen.findByRole('dialog');
  // 会丢未保存的对话：必须先说清楚。
  expect(within(dialog).getByText(/未保存的对话可能丢失/)).toBeInTheDocument();
  expect(restartHost).not.toHaveBeenCalled();

  await user.click(within(dialog).getByRole('button', { name: '重启 Codex' }));
  expect(restartHost).toHaveBeenCalledWith(instance.id);
  expect(await within(notifications()).findByRole('status')).toHaveTextContent(/^Codex 已重启/);
});

test('旧进程没退出去时必须说「没能重启」，绝不报成功', async () => {
  // 回归：早先只要启动命令发出去了就报成功，而 open 对已在运行的 App 只是激活旧进程——
  // 配置没被重读，界面却说重启好了，用户于是去一个没更新的菜单里找模型。
  const user = userEvent.setup();
  const restartHost = vi.fn().mockResolvedValue({ appPath: '/Applications/ChatGPT.app', quitConfirmed: false, quitForced: false, launchedConfirmed: false });
  render(<App client={testClient({ detectInstances: vi.fn().mockResolvedValue([instance]), restartHost })} />);
  await user.click(within(screen.getByRole('navigation')).getByRole('button', { name: '配置' }));
  await screen.findByText('Codex 实例');

  await user.click(screen.getByRole('button', { name: '重启 Codex' }));
  await user.click(within(await screen.findByRole('dialog')).getByRole('button', { name: '重启 Codex' }));

  // 失败走 alert（读屏会立刻打断），成功才是 status。
  const status = await within(notifications()).findByRole('alert');
  expect(status).toHaveTextContent(/Codex 仍在运行，没能重启/);
  expect(status).not.toHaveTextContent(/已重启/);
});

test('退出了但没起来时，说清楚要手动打开', async () => {
  const user = userEvent.setup();
  const restartHost = vi.fn().mockResolvedValue({ appPath: '/Applications/ChatGPT.app', quitConfirmed: true, quitForced: false, launchedConfirmed: false });
  render(<App client={testClient({ detectInstances: vi.fn().mockResolvedValue([instance]), restartHost })} />);
  await user.click(within(screen.getByRole('navigation')).getByRole('button', { name: '配置' }));
  await screen.findByText('Codex 实例');

  await user.click(screen.getByRole('button', { name: '重启 Codex' }));
  await user.click(within(await screen.findByRole('dialog')).getByRole('button', { name: '重启 Codex' }));

  const status = await within(notifications()).findByRole('alert');
  expect(status).toHaveTextContent(/已退出，但没有重新起来/);
  expect(status).not.toHaveTextContent(/已重启/);
});

test('实例检测失败要能看到原因，而不是只显示空态', async () => {
  const user = userEvent.setup();
  render(<App client={testClient({ detectInstances: vi.fn().mockRejectedValue({ code: 'INTERNAL',
    messageKey: 'error.internal', safeDetails: ['无法读取 /Applications'], retryable: false, recoveryActions: [] }) })} />);
  await user.click(within(screen.getByRole('navigation')).getByRole('button', { name: '配置' }));

  // 回归：这条分支过去没有任何错误出口，界面永远停在「未检测到 Codex」，原因看不见。
  expect(await screen.findByRole('alert')).toHaveTextContent('无法读取 /Applications');
  expect(screen.getByText('未检测到 Codex')).toBeInTheDocument();
});

test('提交进行中不能取消：写入已经开始，完成前不能取消', async () => {
  let releaseCommit: () => void = () => {};
  const user = await openCodexPage({
    detectInstances: vi.fn().mockResolvedValue([instance]),
    planApply: vi.fn().mockResolvedValue(plan([modelChange])),
    executeApply: vi.fn().mockImplementation(() => new Promise<void>(resolve => { releaseCommit = () => resolve(); })
      .then(() => ({ operationId: 'op_1' }))),
    applyStatus: vi.fn().mockResolvedValue({ operationId: 'op_1', open: true, events: [event('committing')] }),
  });

  await user.click(screen.getByRole('button', { name: '应用到 Codex' }));
  await user.click(await screen.findByRole('button', { name: '应用并重新加载' }));

  expect(screen.getByRole('button', { name: '取消' })).toBeDisabled();
  expect(screen.getAllByText(/写入已经开始，完成前不能取消/).length).toBeGreaterThan(0);
  releaseCommit();
});

test('未检测到实例时显示安装指引而不显示应用入口', async () => {
  await openEmptyCodexPage();

  expect(screen.getByText('未检测到 Codex')).toBeInTheDocument();
  expect(screen.queryByRole('button', { name: '应用到 Codex' })).not.toBeInTheDocument();
  expect(screen.getByLabelText('Codex 应用路径')).toBeInTheDocument();
});

test('确认之前说清「菜单会被替换」，并列出替换后的模型', async () => {
  // 这是本工具最容易被误解的一条行为：目录是替换整份菜单，不是往里追加。
  // 真机上「加一个模型」的预期与「原来能用的都不见了」的结果对不上，界面必须提前说。
  const listed = { ...plan([modelChange]).catalogAliases };
  const user = await openCodexPage({
    detectInstances: vi.fn().mockResolvedValue([instance]),
    listModels: vi.fn().mockResolvedValue([{
      id: 'm_1', providerId: 'p_test', upstreamId: 'vendor/a', catalogAlias: 'gs/m_1', displayName: '我的模型',
      lifecycle: 'saved', hostState: 'pending_apply', inCatalog: true,
      policy: { contextLimit: 128_000, outputLimit: 8_192, compactLimit: null,
        reasoning: { support: 'unknown', control: 'none', allowedValues: [], defaultValue: null, budgetTokens: null, mappingId: null },
        inputs: [], tools: { functionTools: 'unknown', parallelTools: 'unknown', customTools: 'unknown', verification: 'declared' } },
      displayNameLayer: { discovered: null, userValue: null, overridden: false }, capabilityRevision: 1, version: 1,
      createdAt: '2026-09-18T00:00:00Z', updatedAt: '2026-09-18T00:00:00Z',
    }]),
    planApply: vi.fn().mockResolvedValue(plan([modelChange], { catalogAliases: ['gs/m_1', 'gs/m_2'] })),
  });
  expect(listed).toBeDefined();

  await user.click(screen.getByRole('button', { name: '应用到 Codex' }));

  const note = await screen.findByRole('note');
  expect(within(note).getByText('Codex 的模型菜单会被替换')).toBeInTheDocument();
  expect(within(note).getByText(/还原为原生 Codex/)).toBeInTheDocument();
  expect(within(note).getByText('本次替换后菜单里会有 2 个模型')).toBeInTheDocument();
  // 能对上模型的显示名字；对不上的退回别名，也比什么都不说强。
  expect(within(note).getByText('我的模型')).toBeInTheDocument();
  expect(within(note).getByText('gs/m_2')).toBeInTheDocument();
});

test('还原不显示「替换菜单」警告——它正是把菜单还回去的那个动作', async () => {
  const user = await openCodexPage({
    detectInstances: vi.fn().mockResolvedValue([instance]),
    planRestore: vi.fn().mockResolvedValue(plan([{ keyPath: 'model', before: 'gs/m_1', after: 'gpt-5.6-sol', reasonKey: 'reason.restore' }],
      { catalogAliases: ['gs/m_1'] })),
  });

  // 「还原」按钮点了就直接出差异弹窗（计划生成后即展示），中间没有第二次点击。
  await user.click(screen.getByRole('button', { name: '还原为原生 Codex' }));

  const dialog = await screen.findByRole('dialog');
  expect(within(dialog).getByText('还原差异')).toBeInTheDocument();
  expect(within(dialog).getByText('恢复基线')).toBeInTheDocument();
  expect(screen.queryByText('Codex 的模型菜单会被替换')).not.toBeInTheDocument();
});

/**
 * 共存模式（Bridge）的界面行为。
 *
 * 这里守的是三件事：开关真的调到了核心；**意图与事实分开说**（开着不等于宿主真的跑在
 * bridge 上）；差异弹窗在共存模式下不能再说「菜单会被替换」——那句话在这里是错的。
 */
describe('共存模式', () => {
  const restartHost = vi.fn().mockResolvedValue({
    appPath: '/Applications/ChatGPT.app', quitConfirmed: true, quitForced: false, launchedConfirmed: true,
  });

  test('开启后接着发布模型，并且弹窗说的是「合并」不是「替换」', async () => {
    const setCoexist = vi.fn().mockResolvedValue({ ...coexistOff, enabled: true });
    const planApply = vi.fn().mockResolvedValue(plan([modelChange]));
    const user = await openCodexPage({
      detectInstances: vi.fn().mockResolvedValue([instance]),
      coexistStatus: vi.fn().mockResolvedValue(coexistOff),
      setCoexist,
      planApply,
      restartHost,
    });

    await user.click(await screen.findByRole('button', { name: '开启并应用' }));

    expect(setCoexist).toHaveBeenCalledWith(instance.id, true);
    // 开启之后必须再发布一次：模型要写进托管 profile。
    expect(planApply).toHaveBeenCalled();
    expect(await screen.findByText('菜单里会同时有官方模型和我们发布的模型')).toBeInTheDocument();
    expect(screen.queryByText('Codex 的模型菜单会被替换')).not.toBeInTheDocument();
  });

  test('关闭共存会重启宿主：不重启，正在跑的那个还挂在 bridge 上', async () => {
    const setCoexist = vi.fn().mockResolvedValue({ ...coexistOff, enabled: false });
    const user = await openCodexPage({
      detectInstances: vi.fn().mockResolvedValue([instance]),
      coexistStatus: vi.fn().mockResolvedValue({ ...coexistOff, enabled: true, hostUnderBridge: true }),
      setCoexist,
      restartHost,
    });

    await user.click(await screen.findByRole('button', { name: '关闭并重启 Codex' }));
    expect(setCoexist).toHaveBeenCalledWith(instance.id, false);
    expect(restartHost).toHaveBeenCalledWith(instance.id);
  });

  test('开关开着但宿主不是这样起来的：如实说，不冒充生效', async () => {
    await openCodexPage({
      detectInstances: vi.fn().mockResolvedValue([instance]),
      coexistStatus: vi.fn().mockResolvedValue({ ...coexistOff, enabled: true, hostUnderBridge: false }),
    });
    expect(await screen.findByText(/不是以共存模式启动的/)).toBeInTheDocument();
  });

  test('无法确认时说「无法确认」，不说「没有」', async () => {
    await openCodexPage({
      detectInstances: vi.fn().mockResolvedValue([instance]),
      coexistStatus: vi.fn().mockResolvedValue({ ...coexistOff, enabled: true, hostUnderBridge: null }),
    });
    expect(await screen.findByText(/无法确认当前这个 Codex 是否以共存模式运行/)).toBeInTheDocument();
  });

  test('不具备接管前提时禁用开关并说明原因', async () => {
    const setCoexist = vi.fn();
    await openCodexPage({
      detectInstances: vi.fn().mockResolvedValue([instance]),
      coexistStatus: vi.fn().mockResolvedValue({ ...coexistOff, ready: false, blockedReason: 'error.coexistNeedsCli' }),
      setCoexist,
    });
    expect(await screen.findByText(/共存模式无法接管/)).toBeInTheDocument();
    await userEvent.click(screen.getByRole('button', { name: '开启并应用' }));
    expect(setCoexist).not.toHaveBeenCalled();
  });

  test('共存模式下「还原为原生」没有可还原的内容，按钮点不动', async () => {
    const planRestore = vi.fn();
    await openCodexPage({
      detectInstances: vi.fn().mockResolvedValue([instance]),
      coexistStatus: vi.fn().mockResolvedValue({ ...coexistOff, enabled: true, hostUnderBridge: true }),
      planRestore,
    });
    expect(await screen.findByText(/共存模式下原生配置没有被改动/)).toBeInTheDocument();
    const restore = screen.getByRole('button', { name: /还原为原生/ });
    expect(restore).toBeDisabled();
  });

  test('重新同步：把托管 profile 的底子按当前原生配置重建，然后重新发布', async () => {
    const resyncCoexist = vi.fn().mockResolvedValue({ ...coexistOff, enabled: true, hostUnderBridge: true });
    const planApply = vi.fn().mockResolvedValue(plan([modelChange]));
    const user = await openCodexPage({
      detectInstances: vi.fn().mockResolvedValue([instance]),
      coexistStatus: vi.fn().mockResolvedValue({ ...coexistOff, enabled: true, hostUnderBridge: true }),
      resyncCoexist,
      planApply,
    });

    await user.click(await screen.findByRole('button', { name: /从原生配置重新同步/ }));
    expect(resyncCoexist).toHaveBeenCalledWith(instance.id);
    expect(planApply).toHaveBeenCalled();
  });

  test('不开共存就没有可重建的托管 profile，同步按钮不出现', async () => {
    await openCodexPage({
      detectInstances: vi.fn().mockResolvedValue([instance]),
      coexistStatus: vi.fn().mockResolvedValue(coexistOff),
    });
    await screen.findByRole('heading', { name: /与原生模型共存/ });
    expect(screen.queryByRole('button', { name: /从原生配置重新同步/ })).not.toBeInTheDocument();
  });
});
