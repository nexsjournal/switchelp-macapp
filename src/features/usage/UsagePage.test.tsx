import { screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { expect, it, vi } from 'vitest';
import type { UsageDay, UsageReport, UsageTotals } from '@/contracts/types';
import { UsagePage } from './UsagePage';
import { emptyUsageReport, testClient, zeroTotals } from '../../../tests/helpers/client';
import { renderWithToasts } from '../../../tests/helpers/render';

/**
 * 夹具走的是真实口径：`缓存读取 ⊂ 输入`、`推理 ⊂ 输出`，且 `total` 不由另外两项推算
 * （真机上 `total ≠ input + output` 是存在的），所以这里也刻意让 total 与 input + output 不等
 * （4,061,000 vs 4,060,000）。
 */
function totals(overrides: Partial<UsageTotals> = {}): UsageTotals {
  return { inputTokens: 2_000, cachedTokens: 1_200, cacheWriteTokens: 0, outputTokens: 300, reasoningTokens: 90, totalTokens: 2_305, ...overrides };
}

function day(date: string, overrides: Partial<UsageDay> = {}): UsageDay {
  return { date, sessions: 2, totals: totals(), ...overrides };
}

/** 30 天逐日：前五天覆盖四个分档与空格，其余为零——用来量热力图的分档。 */
function month(): UsageDay[] {
  const peaks = [1_000_000, 600_000, 300_000, 100_000, 0];
  const days: UsageDay[] = [];
  for (let index = 0; index < 30; index++) {
    const value = peaks[index] ?? 0;
    days.push({
      date: `2026-09-${String(index + 1).padStart(2, '0')}`,
      sessions: value > 0 ? 2 : 0,
      totals: value > 0 ? totals({ totalTokens: value }) : zeroTotals(),
    });
  }
  return days;
}

function report(overrides: Partial<UsageReport> = {}): UsageReport {
  return emptyUsageReport({
    sourceDirectory: '/Users/me/.codex',
    scannedFiles: 337,
    sessions: 4,
    // 输入 4,000,000 / 输出 60,000 / 缓存读取 2,100,000 / 推理 24,000，合计 4,061,000。
    totals: totals({ inputTokens: 4_000_000, outputTokens: 60_000, reasoningTokens: 24_000, cachedTokens: 2_100_000, totalTokens: 4_061_000 }),
    daily: [day('2026-09-23'), day('2026-09-24', { totals: totals({ totalTokens: 9_000 }) }), day('2026-09-25', { sessions: 0, totals: zeroTotals() })],
    // 两行的合计与整份报告的总量成整数比（50% / 60% 与 40%），占比取整后能手算核对。
    byModel: [{ model: 'gpt-5.6-sol', sessions: 3, totals: totals({ totalTokens: 2_030_500 }) }],
    byProvider: [
      { provider: 'OpenAI', sessions: 2, totals: totals({ totalTokens: 2_436_600 }) },
      { provider: 'gptswitch', sessions: 2, totals: totals({ totalTokens: 1_624_400 }) },
    ],
    planWindow: { planType: 'plus', usedPercent: 35.4, windowMinutes: 300, resetsAt: 1_790_246_266 },
    ...overrides,
  });
}

it('本机没有任何会话记录时给出空态，并说明扫的是哪个目录', async () => {
  renderWithToasts(<UsagePage client={testClient({ usageReport: vi.fn().mockResolvedValue(emptyUsageReport()) })} />);

  expect(await screen.findByText('还没有会话记录')).toBeInTheDocument();
  expect(screen.getByText(/\/tmp\/gptswitch-test\/\.codex/)).toBeInTheDocument();
  // 空态不给结论带：没有数据时摆一排 0 会像是「读到了但都是零」。
  expect(screen.queryByText('总 Token')).not.toBeInTheDocument();
});

it('结论带：主数字是总 Token，口径行给三个并列口径与完整值', async () => {
  renderWithToasts(<UsagePage client={testClient({ usageReport: vi.fn().mockResolvedValue(report()) })} />);

  const total = await screen.findByText('4.1M');
  expect(total).toHaveAttribute('title', '4,061,000');
  // 紧凑值旁边就是完整值：主数字要能一眼读，也要能核对。
  expect(screen.getByText('4M')).toHaveAttribute('title', '4,000,000');
  expect(screen.getByText('60K')).toHaveAttribute('title', '60,000');
  const cached = screen.getAllByText('2.1M');
  expect(cached.map(node => node.getAttribute('title'))).toEqual(['2,100,000', '2,100,000']);
});

it('派生指标全部按报告现算：日均 / 峰值日 / 每会话 / 缓存命中率', async () => {
  renderWithToasts(<UsagePage client={testClient({ usageReport: vi.fn().mockResolvedValue(report()) })} />);

  // 日均 = 4,061,000 ÷ 30 = 135,366.7 → 135.4K（悬停给不取整的完整值）。
  expect(await screen.findByText('日均 135.4K')).toHaveAttribute('title', '日均 135,367');
  // 峰值日取 daily 里总量最大的那天（09-24 的 9,000）。
  expect(screen.getByText('峰值日 09-24 · 9K')).toHaveAttribute('title', '峰值日 2026-09-24 · 9,000');
  // 每会话 = 4,061,000 ÷ 4 = 1,015,250。
  expect(screen.getByText('每会话 1M')).toHaveAttribute('title', '每会话 1,015,250');
  // 命中率 = 2,100,000 ÷ 4,000,000 = 52.5% → 取整 53%，并写明它是输入内的比例。
  expect(screen.getByText('缓存命中 53%')).toBeInTheDocument();
  expect(screen.getByText('输入中被缓存命中的比例，越高越省')).toBeInTheDocument();
});

it('Token 构成：两条按自身归一化的条，四项不相加', async () => {
  renderWithToasts(<UsagePage client={testClient({ usageReport: vi.fn().mockResolvedValue(report()) })} />);

  const compose = (await screen.findByText('Token 构成')).closest('section')!;
  // 缓存命中：2,100,000 ÷ 4,000,000 = 52.5% → 53%（与上面的命中率同一个数，口径必须一致）。
  expect(within(compose).getByText('缓存读取')).toBeInTheDocument();
  expect(within(compose).getByText('含在输入内')).toBeInTheDocument();
  // 推理：24,000 ÷ 60,000 = 40%（排行里另有一行也是 40%，所以按卡片范围查）。
  expect(within(compose).getByText('40%')).toBeInTheDocument();
  // 说明必须写明四项不相加，而不是留给读者自己推。
  expect(screen.getByText(/四项不相加/)).toBeInTheDocument();
  // 输入 + 输出的和（4,060,000）在页面上任何地方都不能出现：真机上 total 与它本来就不相等。
  expect(screen.queryByText(/4,060,000/)).not.toBeInTheDocument();
  // 更根本的一条：四个字段各自从报告算，页面上不存在把两项相加的式子。
  expect(document.body.textContent).not.toContain('4,060,000');
});

it('全页不出现成本、金额或汇率：没有可靠单价来源就不给钱', async () => {
  renderWithToasts(<UsagePage client={testClient({ usageReport: vi.fn().mockResolvedValue(report()) })} />);

  await screen.findByText('总 Token');
  const text = document.body.textContent ?? '';
  for (const banned of ['成本', '金额', '费用', '¥', '$', 'USD', '美元']) {
    expect(text).not.toContain(banned);
  }
});

it('近 30 天用热力图：一天一格，分档随用量变化，另附少 / 多图例', async () => {
  renderWithToasts(<UsagePage client={testClient({ usageReport: vi.fn().mockResolvedValue(report({ daily: month() })) })} />);

  const grid = await screen.findByRole('img', { name: '每日 Token 用量热力图' });
  const cells = [...grid.querySelectorAll('[data-level]')];
  // 逐日零填充：范围内 30 天就是 30 格，一格不多一格不少。
  expect(cells).toHaveLength(30);
  // 分档相对本范围峰值（1,000,000）：100% / 60% / 30% / 10% / 0% 依次落到 4 / 3 / 2 / 1 / 0 档。
  expect(cells.slice(0, 5).map(cell => cell.getAttribute('data-level'))).toEqual(['4', '3', '2', '1', '0']);
  // 逐日数字走 title：日期 · 完整用量 · 会话数。
  expect(cells[0]).toHaveAttribute('title', '2026-09-01 · 1,000,000 · 会话 2 个');
  expect(screen.getByText('少')).toBeInTheDocument();
  expect(screen.getByText('多')).toBeInTheDocument();
  // 30 天里不出现柱状图：两种图形互斥，同一份数据不会画两遍。
  expect(screen.queryByRole('img', { name: '每日 Token 用量柱状图' })).not.toBeInTheDocument();
});

it('近 7 天仍是柱状图：天数少，柱子还看得清', async () => {
  const usageReport = vi.fn().mockResolvedValue(report({ rangeDays: 7, daily: month().slice(23) }));
  renderWithToasts(<UsagePage client={testClient({ usageReport })} />);

  // 默认 30 天是热力图，切到 7 天才换成柱状图。峰值那行仍由页面拼好交给图。
  await screen.findByRole('img', { name: '每日 Token 用量热力图' });
  await userEvent.click(screen.getByRole('tab', { name: '近 7 天' }));

  expect(await screen.findByRole('img', { name: '每日 Token 用量柱状图' })).toBeInTheDocument();
  expect(screen.queryByRole('img', { name: '每日 Token 用量热力图' })).not.toBeInTheDocument();
});

it('按模型与按供应商两张表的表头都带 scope，占比按总量算', async () => {
  renderWithToasts(<UsagePage client={testClient({ usageReport: vi.fn().mockResolvedValue(report()) })} />);

  await screen.findByText('gpt-5.6-sol');
  const tables = screen.getAllByRole('table');
  expect(tables).toHaveLength(2);
  for (const table of tables) {
    const headers = within(table).getAllByRole('columnheader');
    expect(headers.length).toBeGreaterThan(0);
    for (const header of headers) expect(header).toHaveAttribute('scope', 'col');
  }
  // 占比以整份报告的总量为分母，两张卡横着能比：模型 2,030,500 / 4,061,000 = 50%。
  expect(within(tables[0]!).getByText('50%')).toBeInTheDocument();
  expect(within(tables[0]!).getByText('2M')).toHaveAttribute('title', '2,030,500');
  expect(within(tables[1]!).getByText('60%')).toBeInTheDocument();
  expect(within(tables[1]!).getByText('40%')).toBeInTheDocument();
  expect(within(tables[1]!).getByText('gptswitch')).toBeInTheDocument();
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

it('计划额度：读得到时环形按取整后的百分比画、时长按时长说；读不到时整卡换成说明', async () => {
  const { unmount } = renderWithToasts(
    <UsagePage client={testClient({ usageReport: vi.fn().mockResolvedValue(report()) })} />,
  );
  // 真实数据里窗口不都是天：本机最新一条是 300 分钟，必须说「5 小时」而不是「0 天」。
  expect(await screen.findByText(/窗口 5 小时/)).toBeInTheDocument();
  expect(screen.getByText(/计划 plus/)).toBeInTheDocument();
  // 35.4% 取整成 35%，不显示小数位；环按同一个数画（314.159 × (1 − 0.35) ≈ 204.2）。
  const planCard = screen.getByText('本机计划额度').closest('section')!;
  const ring = [...planCard.querySelectorAll('svg circle')];
  expect(ring).toHaveLength(2);
  expect(Number(ring[1]!.getAttribute('stroke-dashoffset'))).toBeCloseTo(204.2, 0);
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

it('本机有会话但所选范围没数据时，结论带仍渲染并给出一句范围提示', async () => {
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
  // 峰值日：整天都没有用量时不给一个日期（0 没有峰值），也不编一个日期出来。
  expect(screen.getByText('日均 0')).toBeInTheDocument();
  expect(screen.queryByText(/峰值日/)).not.toBeInTheDocument();
  // 两张表各有一句空提示，不是重复渲染。
  expect(screen.getAllByText('这个范围内没有记录。')).toHaveLength(2);
});
