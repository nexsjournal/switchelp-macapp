import { screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { expect, it, vi } from 'vitest';import type { UsageDay, UsageReport, UsageTotals } from '@/contracts/types';
import { UsagePage } from './UsagePage';
import { emptyUsageReport, testClient, zeroTotals } from '../../../tests/helpers/client';
import { renderWithToasts } from '../../../tests/helpers/render';

/**
 * 夹具走的是真实口径：`缓存读取 ⊂ 输入`、`输出 ⊂ 总量的一部分`，且 `total` 不由另外两项推算
 * （真机上 `total ≠ input + output` 是存在的），所以这里也刻意让 total 与 input + output 不等。
 */
function totals(overrides: Partial<UsageTotals> = {}): UsageTotals {
  return { inputTokens: 2_000, cachedTokens: 1_200, cacheWriteTokens: 0, outputTokens: 300, reasoningTokens: 90, totalTokens: 2_305, ...overrides };
}

function day(date: string, overrides: Partial<UsageDay> = {}): UsageDay {
  return { date, sessions: 2, totals: totals(), ...overrides };
}

function report(overrides: Partial<UsageReport> = {}): UsageReport {
  return emptyUsageReport({
    sourceDirectory: '/Users/me/.codex',
    scannedFiles: 337,
    sessions: 4,
    totals: totals({ inputTokens: 4_000_000, outputTokens: 60_000, cachedTokens: 2_100_000, totalTokens: 4_061_000 }),
    daily: [day('2026-09-23'), day('2026-09-24', { totals: totals({ totalTokens: 9_000 }) }), day('2026-09-25', { sessions: 0, totals: zeroTotals() })],
    byModel: [{ model: 'gpt-5.6-sol', sessions: 3, totals: totals() }],
    byProvider: [{ provider: 'OpenAI', sessions: 2, totals: totals() }, { provider: 'gptswitch', sessions: 2, totals: totals() }],
    planWindow: { planType: 'plus', usedPercent: 35.4, windowMinutes: 300, resetsAt: 1_790_246_266 },
    ...overrides,
  });
}

it('本机没有任何会话记录时给出空态，并说明扫的是哪个目录', async () => {
  renderWithToasts(<UsagePage client={testClient({ usageReport: vi.fn().mockResolvedValue(emptyUsageReport()) })} />);

  expect(await screen.findByText('还没有会话记录')).toBeInTheDocument();
  expect(screen.getByText(/\/tmp\/gptswitch-test\/\.codex/)).toBeInTheDocument();
  // 空态不给指标卡：没有数据时摆四个 0 会像是「读到了但都是零」。
  expect(screen.queryByText('总 Token')).not.toBeInTheDocument();
});

it('有数据时四个指标按紧凑记法显示，完整值放在 title 里', async () => {
  renderWithToasts(<UsagePage client={testClient({ usageReport: vi.fn().mockResolvedValue(report()) })} />);

  const total = await screen.findByText('4.1M');
  expect(total).toHaveAttribute('title', '4,061,000');
  expect(screen.getByText('4M')).toHaveAttribute('title', '4,000,000');
  expect(screen.getByText('2.1M')).toHaveAttribute('title', '2,100,000');
  expect(screen.getByText('60K')).toHaveAttribute('title', '60,000');
  // 缓存读取写在输入内，界面必须说明这种包含关系。
  expect(screen.getByText('含在输入内')).toBeInTheDocument();
});

it('按模型与按供应商两张表的表头都带 scope，行数与数据一致', async () => {
  renderWithToasts(<UsagePage client={testClient({ usageReport: vi.fn().mockResolvedValue(report()) })} />);

  await screen.findByText('gpt-5.6-sol');
  const tables = screen.getAllByRole('table');
  expect(tables).toHaveLength(2);
  for (const table of tables) {
    const headers = within(table).getAllByRole('columnheader');
    expect(headers.length).toBeGreaterThan(0);
    for (const header of headers) expect(header).toHaveAttribute('scope', 'col');
  }
  expect(within(tables[0]!).getByRole('cell', { name: 'gpt-5.6-sol' })).toBeInTheDocument();
  expect(within(tables[1]!).getByRole('cell', { name: 'gptswitch' })).toBeInTheDocument();
});

it('切换时间范围会按 7 / 30 / 90 重新请求', async () => {
  const usageReport = vi.fn().mockResolvedValue(report());
  renderWithToasts(<UsagePage client={testClient({ usageReport })} />);

  await waitFor(() => expect(usageReport).toHaveBeenCalledWith(30));
  await userEvent.click(screen.getByRole('tab', { name: '近 7 天' }));
  await waitFor(() => expect(usageReport).toHaveBeenCalledWith(7));
  await userEvent.click(screen.getByRole('tab', { name: '近 90 天' }));
  await waitFor(() => expect(usageReport).toHaveBeenCalledWith(90));
  // 分段控件是 role=tab，不是裸按钮组。
  expect(screen.getAllByRole('tab')).toHaveLength(3);
});

it('计划额度：读得到时按时长说窗口、百分比取整；读不到时整卡换成说明', async () => {
  const { unmount } = renderWithToasts(
    <UsagePage client={testClient({ usageReport: vi.fn().mockResolvedValue(report()) })} />,
  );
  // 真实数据里窗口不都是天：本机最新一条是 300 分钟，必须说「5 小时」而不是「0 天」。
  expect(await screen.findByText(/窗口 5 小时/)).toBeInTheDocument();
  expect(screen.getByText(/计划 plus/)).toBeInTheDocument();
  // 35.4% 取整成 35%，不显示小数位。
  expect(screen.getByText('已用 35%')).toBeInTheDocument();
  unmount();

  renderWithToasts(
    <UsagePage client={testClient({ usageReport: vi.fn().mockResolvedValue(report({ planWindow: null })) })} />,
  );
  expect(await screen.findByText(/没有读到计划额度/)).toBeInTheDocument();
  expect(screen.queryByText(/已用 /)).not.toBeInTheDocument();
});

it('有文件读不了时证据行如实写出数量', async () => {
  const { unmount } = renderWithToasts(
    <UsagePage client={testClient({ usageReport: vi.fn().mockResolvedValue(report({ unreadableFiles: 3 })) })} />,
  );
  expect(await screen.findByText(/337 个文件/)).toBeInTheDocument();
  expect(screen.getByText(/3 个不可读/)).toBeInTheDocument();
  unmount();

  // 一个都读不了时不提「0 个不可读」：那句话没有信息量。
  renderWithToasts(<UsagePage client={testClient({ usageReport: vi.fn().mockResolvedValue(report()) })} />);
  expect(await screen.findByText(/337 个文件/)).toBeInTheDocument();
  expect(screen.queryByText(/个不可读/)).not.toBeInTheDocument();
});

it('扫描失败时页面内联报错，错误只出现一次（不弹 Toast）', async () => {
  const usageReport = vi.fn().mockRejectedValue({ code: 'INTERNAL', messageKey: 'error.internal', safeDetails: ['扫描失败：权限不足'], retryable: false, recoveryActions: [] });
  renderWithToasts(<UsagePage client={testClient({ usageReport })} />);

  const alert = await screen.findByRole('alert');
  expect(alert).toHaveTextContent('扫描失败：权限不足');
  // 页面级状态留在页面里；只有动作结果才走 Toast。
  expect(screen.queryByRole('status')).not.toBeInTheDocument();
});

it('「重新扫描」按当前范围重新请求并给出提示', async () => {
  const usageReport = vi.fn().mockResolvedValue(report());
  renderWithToasts(<UsagePage client={testClient({ usageReport })} />);

  await waitFor(() => expect(usageReport).toHaveBeenCalledTimes(1));
  await userEvent.click(screen.getByRole('button', { name: /重新扫描/ }));

  expect(await screen.findByText('已重新扫描本机会话记录')).toBeInTheDocument();
  await waitFor(() => expect(usageReport).toHaveBeenCalledTimes(2));
  expect(usageReport).toHaveBeenLastCalledWith(30);
});

it('本机有会话但所选范围没数据时，指标卡仍渲染并给出一句范围提示', async () => {
  renderWithToasts(
    <UsagePage client={testClient({ usageReport: vi.fn().mockResolvedValue(report({
      sessions: 0,
      totals: zeroTotals(),
      daily: [day('2026-09-25', { sessions: 0, totals: zeroTotals() })],
      byModel: [],
      byProvider: [],
      planWindow: null,
    })) })} />,
  );

  expect(await screen.findByText('总 Token')).toBeInTheDocument();
  expect(screen.getByText(/近 30 天没有记录/)).toBeInTheDocument();
  // 两张表各有一句空提示，不是重复渲染。
  expect(screen.getAllByText('这个范围内没有记录。')).toHaveLength(2);
});
