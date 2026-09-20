import { render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import type { ApplyStage, FieldChange, OperationEvent } from '@/contracts/types';
import { App } from '@/app/App';
import { instance, plan, testClient } from '../../../tests/helpers/client';

function event(phase: ApplyStage): OperationEvent {
  return { schemaVersion: 1, operationId: 'op_1', sequence: 1, phase, revisionId: 'rev_test',
    messageKey: `stage.${phase}`, safeArgs: {}, cancellable: false, timestamp: '2026-09-18T00:00:00Z' };
}

const modelChange: FieldChange = { keyPath: 'model', before: null, after: 'p_test-alias', reasonKey: 'reason.defaultModel' };
const providerChange: FieldChange = { keyPath: 'model_providers.gptswitch', before: null, after: 'base_url = …', reasonKey: 'reason.gatewayProvider' };

async function openCodexPage(client: Parameters<typeof testClient>[0]) {
  const user = userEvent.setup();
  render(<App client={testClient(client)} />);
  await user.click(within(screen.getByRole('navigation')).getByRole('button', { name: 'Codex 配置' }));
  await screen.findByText('Codex 实例');
  return user;
}

/** 未检测到实例时页面不渲染实例卡片，需要单独进入。 */
async function openEmptyCodexPage() {
  const user = userEvent.setup();
  render(<App client={testClient({ detectInstances: vi.fn().mockResolvedValue([]) })} />);
  await user.click(within(screen.getByRole('navigation')).getByRole('button', { name: 'Codex 配置' }));
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
  expect(screen.getByRole('status')).toHaveTextContent(/配置已提交，Codex 已重启/);
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

test('外部修改导致 CAS 冲突时给出重新比较入口，并重新生成计划', async () => {
  const planApply = vi.fn()
    .mockResolvedValueOnce(plan([modelChange]))
    .mockResolvedValueOnce(plan([modelChange], { id: 'plan_retry' }));
  const user = await openCodexPage({
    detectInstances: vi.fn().mockResolvedValue([instance]),
    planApply,
    executeApply: vi.fn().mockRejectedValue({ code: 'CONFIG_CHANGED', messageKey: 'error.configChanged',
      safeDetails: ['配置文件在计划生成后被其他程序修改'], retryable: false, recoveryActions: [{ action: 'recompare', messageKey: 'action.recompare' }] }),
    applyStatus: vi.fn().mockResolvedValue({ operationId: 'op_1', open: false, events: [event('conflict')] }),
  });

  await user.click(screen.getByRole('button', { name: '应用到 Codex' }));
  await user.click(screen.getByRole('button', { name: '应用并重新加载' }));

  expect(await screen.findByRole('alert')).toHaveTextContent('配置文件在计划生成后被其他程序修改');
  // 失败的计划不得被当成已提交状态展示。
  expect(screen.queryByText('Codex 已重新加载并核验本次目录。')).not.toBeInTheDocument();
  await user.click(screen.getByRole('button', { name: '重新比较' }));
  expect(planApply).toHaveBeenCalledTimes(2);
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
  await user.click(within(screen.getByRole('navigation')).getByRole('button', { name: 'Codex 配置' }));
  await screen.findByText('Codex 实例');

  await user.click(screen.getByRole('button', { name: '重启 Codex' }));
  const dialog = await screen.findByRole('dialog');
  // 会丢未保存的对话：必须先说清楚。
  expect(within(dialog).getByText(/未保存的对话可能丢失/)).toBeInTheDocument();
  expect(restartHost).not.toHaveBeenCalled();

  await user.click(within(dialog).getByRole('button', { name: '重启 Codex' }));
  expect(restartHost).toHaveBeenCalledWith(instance.id);
  expect(await screen.findByRole('status')).toHaveTextContent(/^Codex 已重启/);
});

test('旧进程没退出去时必须说「没能重启」，绝不报成功', async () => {
  // 回归：早先只要启动命令发出去了就报成功，而 open 对已在运行的 App 只是激活旧进程——
  // 配置没被重读，界面却说重启好了，用户于是去一个没更新的菜单里找模型。
  const user = userEvent.setup();
  const restartHost = vi.fn().mockResolvedValue({ appPath: '/Applications/ChatGPT.app', quitConfirmed: false, quitForced: false, launchedConfirmed: false });
  render(<App client={testClient({ detectInstances: vi.fn().mockResolvedValue([instance]), restartHost })} />);
  await user.click(within(screen.getByRole('navigation')).getByRole('button', { name: 'Codex 配置' }));
  await screen.findByText('Codex 实例');

  await user.click(screen.getByRole('button', { name: '重启 Codex' }));
  await user.click(within(await screen.findByRole('dialog')).getByRole('button', { name: '重启 Codex' }));

  const status = await screen.findByRole('status');
  expect(status).toHaveTextContent(/Codex 仍在运行，没能重启/);
  expect(status).not.toHaveTextContent(/已重启/);
});

test('退出了但没起来时，说清楚要手动打开', async () => {
  const user = userEvent.setup();
  const restartHost = vi.fn().mockResolvedValue({ appPath: '/Applications/ChatGPT.app', quitConfirmed: true, quitForced: false, launchedConfirmed: false });
  render(<App client={testClient({ detectInstances: vi.fn().mockResolvedValue([instance]), restartHost })} />);
  await user.click(within(screen.getByRole('navigation')).getByRole('button', { name: 'Codex 配置' }));
  await screen.findByText('Codex 实例');

  await user.click(screen.getByRole('button', { name: '重启 Codex' }));
  await user.click(within(await screen.findByRole('dialog')).getByRole('button', { name: '重启 Codex' }));

  const status = await screen.findByRole('status');
  expect(status).toHaveTextContent(/已退出，但没有重新起来/);
  expect(status).not.toHaveTextContent(/已重启/);
});

test('实例检测失败要能看到原因，而不是只显示空态', async () => {
  const user = userEvent.setup();
  render(<App client={testClient({ detectInstances: vi.fn().mockRejectedValue({ code: 'INTERNAL',
    messageKey: 'error.internal', safeDetails: ['无法读取 /Applications'], retryable: false, recoveryActions: [] }) })} />);
  await user.click(within(screen.getByRole('navigation')).getByRole('button', { name: 'Codex 配置' }));

  // 回归：这条分支过去没有任何错误出口，界面永远停在「未检测到 Codex」，原因看不见。
  expect(await screen.findByRole('alert')).toHaveTextContent('无法读取 /Applications');
  expect(screen.getByText('未检测到 Codex')).toBeInTheDocument();
});

test('提交进行中不能取消：写入不可安全中断', async () => {
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
  expect(screen.getAllByText(/写入不可安全中断/).length).toBeGreaterThan(0);
  releaseCommit();
});

test('未检测到实例时显示安装指引而不显示应用入口', async () => {
  await openEmptyCodexPage();

  expect(screen.getByText('未检测到 Codex')).toBeInTheDocument();
  expect(screen.queryByRole('button', { name: '应用到 Codex' })).not.toBeInTheDocument();
  expect(screen.getByLabelText('Codex 应用路径')).toBeInTheDocument();
});
