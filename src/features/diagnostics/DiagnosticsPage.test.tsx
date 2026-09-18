import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { DiagnosticsPage } from './DiagnosticsPage';
import { testClient } from '../../../tests/helpers/client';

const warningEvent = {
  timestamp: '2026-09-18T00:00:00Z',
  level: 'warning' as const,
  categoryKey: 'gateway',
  targetLabel: 'gs/p_a/m_1',
  resultKey: 'result.upstreamFailed',
  elapsedMs: 412,
  safeMetadata: { http_status: '502', provider_id: 'p_a' },
};

test('诊断页说明收集边界，并展示脱敏事件', async () => {
  const client = testClient({ listDiagnostics: vi.fn().mockResolvedValue({ items: [warningEvent], nextCursor: null }) });
  render(<DiagnosticsPage client={client} />);

  expect(await screen.findByText('result.upstreamFailed')).toBeInTheDocument();
  expect(screen.getByText('gs/p_a/m_1')).toBeInTheDocument();
  expect(screen.getByText(/http_status=502/)).toBeInTheDocument();
  expect(screen.getByText(/prompt、completion、工具参数/)).toBeInTheDocument();
});

test('导出前必须先预览，且预览要列出被排除的敏感项', async () => {
  const user = userEvent.setup();
  const exportDiagnostics = vi.fn().mockResolvedValue({ savedPath: '/tmp/exports/diagnostics.json' });
  const client = testClient({
    listDiagnostics: vi.fn().mockResolvedValue({ items: [], nextCursor: null }),
    previewDiagnostics: vi.fn().mockResolvedValue({
      totalBytes: 2048,
      items: [
        { name: 'diagnostics.json', included: true, note: '2 条事件' },
        { name: '上游 API Key 与网关令牌', included: false, note: '永不写入：只记录凭据引用与版本号' },
      ],
    }),
    exportDiagnostics,
  });
  render(<DiagnosticsPage client={client} />);

  const save = await screen.findByRole('button', { name: /保存到本地/ });
  expect(save).toBeDisabled();
  expect(exportDiagnostics).not.toHaveBeenCalled();

  await user.click(screen.getByRole('button', { name: '生成预览' }));

  expect(await screen.findByText('上游 API Key 与网关令牌')).toBeInTheDocument();
  expect(screen.getByText('不包含')).toBeInTheDocument();
  expect(screen.getByText(/2,048 字节/)).toBeInTheDocument();

  await user.click(save);
  expect(exportDiagnostics).toHaveBeenCalledWith(expect.objectContaining({
    scopes: expect.arrayContaining(['gateway', 'apply', 'probe', 'discovery']),
  }));
  expect(await screen.findByText('/tmp/exports/diagnostics.json')).toBeInTheDocument();
});

test('级别过滤会传给后端，而不是在前端本地过滤', async () => {
  const user = userEvent.setup();
  const listDiagnostics = vi.fn().mockResolvedValue({ items: [], nextCursor: null });
  render(<DiagnosticsPage client={testClient({ listDiagnostics })} />);

  await user.selectOptions(await screen.findByLabelText('日志级别'), 'error');

  expect(listDiagnostics).toHaveBeenLastCalledWith({ level: 'error' });
});
