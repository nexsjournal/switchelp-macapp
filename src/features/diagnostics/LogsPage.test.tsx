import { screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { LogsPage } from './LogsPage';
import { testClient } from '../../../tests/helpers/client';
import { renderWithToasts } from '../../../tests/helpers/render';

const warningEvent = {
  // 相对当前时间：时间范围筛选依赖真实时钟，写死日期会随时钟流逝而失效。
  timestamp: new Date(Date.now() - 60_000).toISOString(),
  level: 'warning' as const,
  categoryKey: 'gateway',
  targetLabel: 'gs/p_a/m_1',
  resultKey: 'result.upstreamFailed',
  elapsedMs: 412,
  safeMetadata: { http_status: '502', provider_id: 'p_a' },
};

const olderApplyEvent = {
  timestamp: '2020-01-01T00:00:00Z',
  level: 'info' as const,
  categoryKey: 'apply',
  targetLabel: 'inst_1',
  resultKey: 'result.oldApply',
  elapsedMs: null,
  safeMetadata: {},
};

function logsClient(overrides = {}) {
  return testClient({
    listDiagnostics: vi.fn().mockResolvedValue({ items: [olderApplyEvent, warningEvent], nextCursor: null }),
    ...overrides,
  });
}

test('说明收集边界，并把事件渲染成可点开的行', async () => {
  renderWithToasts(<LogsPage client={logsClient()} />);

  expect(await screen.findByText('result.upstreamFailed')).toBeInTheDocument();
  expect(screen.getByText('gs/p_a/m_1')).toBeInTheDocument();
  expect(screen.getByText(/prompt、completion、工具参数/)).toBeInTheDocument();
  expect(screen.getByRole('button', { name: '查看事件详情 result.upstreamFailed' })).toBeInTheDocument();
});


test('改动导出范围后预览作废，避免导出与预览过的清单不一致', async () => {
  const user = userEvent.setup();
  const previewDiagnostics = vi.fn().mockResolvedValue({ items: [], totalBytes: 0 });
  const exportDiagnostics = vi.fn().mockResolvedValue({ savedPath: '/tmp/diagnostics.json' });
  renderWithToasts(<LogsPage client={testClient({ listDiagnostics: vi.fn().mockResolvedValue({ items: [warningEvent], nextCursor: null }),
    previewDiagnostics, exportDiagnostics })} />);
  await screen.findByText('result.upstreamFailed');

  await user.click(screen.getByRole('button', { name: '生成预览' }));
  const save = await screen.findByRole('button', { name: /保存到本地/ });
  expect(save).toBeEnabled();

  // 预览之后取消勾选一类：清单已经不代表将要导出的内容，必须先重新预览。
  await user.click(screen.getByRole('checkbox', { name: 'gateway' }));
  expect(screen.getByRole('button', { name: /保存到本地/ })).toBeDisabled();
  expect(exportDiagnostics).not.toHaveBeenCalled();
});

test('级别过滤传给后端，类别与时间只影响展示', async () => {
  const user = userEvent.setup();
  const listDiagnostics = vi.fn().mockResolvedValue({ items: [olderApplyEvent, warningEvent], nextCursor: null });
  renderWithToasts(<LogsPage client={testClient({ listDiagnostics })} />);
  await screen.findByText('result.upstreamFailed');

  await user.selectOptions(screen.getByLabelText('日志级别'), 'error');
  expect(listDiagnostics).toHaveBeenLastCalledWith({ level: 'error' });

  // 类别筛选是前端行为，不应再打一次后端。
  const calls = listDiagnostics.mock.calls.length;
  await user.selectOptions(screen.getByLabelText('日志类别'), 'apply');
  expect(screen.getByText('result.oldApply')).toBeInTheDocument();
  expect(screen.queryByText('result.upstreamFailed')).not.toBeInTheDocument();
  expect(listDiagnostics.mock.calls.length).toBe(calls);

  await user.selectOptions(screen.getByLabelText('日志类别'), 'all');
  await user.selectOptions(screen.getByLabelText('时间范围'), '1h');
  expect(screen.queryByText('result.oldApply')).not.toBeInTheDocument();
  expect(screen.getByText('result.upstreamFailed')).toBeInTheDocument();
});

test('事件详情展示安全元数据与关联事件', async () => {
  const user = userEvent.setup();
  renderWithToasts(<LogsPage client={logsClient()} />);
  await screen.findByText('result.upstreamFailed');

  await user.click(screen.getByRole('button', { name: '查看事件详情 result.upstreamFailed' }));
  const dialog = await screen.findByRole('dialog');

  expect(within(dialog).getByText('http_status')).toBeInTheDocument();
  expect(within(dialog).getByText('502')).toBeInTheDocument();
  expect(within(dialog).getByText('安全元数据')).toBeInTheDocument();
  expect(within(dialog).getByText('关联事件')).toBeInTheDocument();
});

test('清空日志要确认，并明确只影响本工具记录', async () => {
  const user = userEvent.setup();
  const clearDiagnostics = vi.fn().mockResolvedValue(2);
  renderWithToasts(<LogsPage client={logsClient({ clearDiagnostics })} />);
  await screen.findByText('result.upstreamFailed');

  await user.click(screen.getByRole('button', { name: /清空日志/ }));
  const dialog = await screen.findByRole('dialog');
  expect(within(dialog).getByText(/不影响 Codex 会话历史，也不影响配置记录与备份/)).toBeInTheDocument();
  expect(clearDiagnostics).not.toHaveBeenCalled();

  await user.click(within(dialog).getByRole('button', { name: '清空日志' }));
  expect(clearDiagnostics).toHaveBeenCalled();
  expect(await screen.findByText(/已清空 2 条本工具诊断事件/)).toBeInTheDocument();
});

test('导出前必须先预览，预览要列出被排除的敏感项', async () => {
  const user = userEvent.setup();
  const exportDiagnostics = vi.fn().mockResolvedValue({ savedPath: '/tmp/exports/diagnostics.json' });
  renderWithToasts(<LogsPage client={logsClient({
    previewDiagnostics: vi.fn().mockResolvedValue({
      totalBytes: 2048,
      items: [
        { name: 'diagnostics.json', included: true, note: '2 条事件' },
        { name: '上游 API Key 与网关令牌', included: false, note: '永不写入：只记录凭据引用与版本号' },
      ],
    }),
    exportDiagnostics,
  })} />);
  await screen.findByText('result.upstreamFailed');

  const save = screen.getByRole('button', { name: /保存到本地/ });
  expect(save).toBeDisabled();

  await user.click(screen.getByRole('button', { name: '生成预览' }));
  expect(await screen.findByText('上游 API Key 与网关令牌')).toBeInTheDocument();
  expect(screen.getByText('不包含')).toBeInTheDocument();

  await user.click(save);
  expect(exportDiagnostics).toHaveBeenCalledWith(expect.objectContaining({
    scopes: expect.arrayContaining(['gateway', 'apply', 'probe', 'discovery']),
  }));
  expect(await screen.findByText('/tmp/exports/diagnostics.json')).toBeInTheDocument();
});
