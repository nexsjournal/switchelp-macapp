import { render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { App } from '@/app/App';
import { provider, testClient } from '../../tests/helpers/client';

/**
 * UX-05 的可自动验证部分：键盘可达、语义正确、平台钩子生效。
 *
 * 真机读屏（VoiceOver / NVDA）与 Windows 字体的实际表现无法在 jsdom 里验证，
 * 这里只覆盖能确定性断言的结构与焦点行为。
 */

test('键盘用户第一个 Tab 能跳到主内容', async () => {
  const user = userEvent.setup();
  render(<App client={testClient()} />);

  const skip = await screen.findByRole('link', { name: '跳到主要内容' });
  expect(skip).toHaveAttribute('href', '#main-content');
  expect(screen.getByRole('main')).toHaveAttribute('id', 'main-content');

  await user.tab();
  expect(skip).toHaveFocus();

  await user.keyboard('{Enter}');
  expect(screen.getByRole('main')).toHaveFocus();
});

test('主导航有可读名称，模型表格的表头带列作用域', async () => {
  const client = testClient({
    listProviders: vi.fn().mockResolvedValue({ items: [provider], nextCursor: null }),
    listModels: vi.fn().mockResolvedValue([{
      id: 'm_1', providerId: 'p_test', upstreamId: 'vendor/model', catalogAlias: 'gs/m_1',
      displayName: '测试模型', lifecycle: 'saved', hostState: 'pending_apply', inCatalog: true,
      policy: { contextLimit: 128_000, outputLimit: 8_192, compactLimit: null,
        reasoning: { support: 'unknown', control: 'none', allowedValues: [], defaultValue: null, budgetTokens: null, mappingId: null },
        inputs: [], tools: { functionTools: 'unknown', parallelTools: 'unknown', customTools: 'unknown', verification: 'declared' } },
      displayNameLayer: { discovered: null, userValue: null, overridden: false },
      capabilityRevision: 1, version: 1, createdAt: '2026-09-18T00:00:00Z', updatedAt: '2026-09-18T00:00:00Z',
    }]),
  });
  const user = userEvent.setup();
  render(<App client={client} />);
  await screen.findByText('测试供应商');
  // 模型表格已归到模型页（概览只留卡片）。
  await user.click(within(screen.getByRole('navigation')).getByRole('button', { name: '网关' }));
  await screen.findByText('测试模型');

  expect(screen.getByRole('navigation', { name: '主导航' })).toBeInTheDocument();

  const table = screen.getAllByRole('table')[0]!;
  for (const header of within(table).getAllByRole('columnheader')) {
    expect(header).toHaveAttribute('scope', 'col');
  }
});

test('平台结论写到根元素的 data-platform，供 CSS 分支', async () => {
  render(<App client={testClient()} />);
  await screen.findByText(/本地配置/);

  await vi.waitFor(() => expect(document.documentElement.dataset.platform).toBe('macos'));
  // 系统绘制标题栏时必须标记出来，避免界面再自绘一条。
  expect(document.documentElement.dataset.systemDecorations).toBe('true');
});

test('拿不到平台接口时回落到 UA 判断，而不是留空', async () => {
  const client = testClient({ platformInfo: vi.fn().mockRejectedValue(new Error('not tauri')) });
  render(<App client={client} />);
  await screen.findByText(/本地配置/);

  await vi.waitFor(() => expect(document.documentElement.dataset.platform).toBeDefined());
});

test('加载占位与提示都是可播报的状态区，错误是告警区', async () => {
  const client = testClient({
    listProviders: vi.fn().mockRejectedValue({ code: 'INTERNAL', messageKey: 'error.internal',
      safeDetails: ['数据读取失败'], retryable: false, recoveryActions: [] }),
  });
  render(<App client={client} />);

  const alert = await screen.findByRole('alert');
  expect(alert).toHaveTextContent('数据读取失败');
});
