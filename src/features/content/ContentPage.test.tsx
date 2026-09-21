import { screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import type { ContentStatus, FeedItem, FeedSource, RefreshReport } from '@/contracts/types';
import { ContentPage } from './ContentPage';
import { testClient } from '../../../tests/helpers/client';
import { renderWithToasts } from '../../../tests/helpers/render';

const NOW = Math.floor(Date.now() / 1000);

const source = (overrides: Partial<FeedSource> & { id: string }): FeedSource => ({
  kind: 'rss', url: `https://${overrides.id}.test/feed`, label: overrides.id, lang: 'zh', enabled: true,
  etag: null, lastModified: null, lastOkAt: NOW - 600, lastError: null, failStreak: 0,
  nextFetchAt: NOW + 1800, builtin: true, ...overrides,
});

const news = (id: string, overrides: Partial<FeedItem> = {}): FeedItem => ({
  url: `https://sspai.test/${id}`, sourceId: 'sspai', sourceLabel: '少数派', title: `资讯 ${id}`,
  summary: '摘要', publishedAt: NOW - 1200, firstSeenAt: NOW - 1200, lang: 'zh', stars: null, repo: null, ...overrides,
});

const status = (overrides: Partial<ContentStatus> = {}): ContentStatus => ({
  lastOkAt: NOW - 600, nextFetchAt: NOW + 1800, scheduleHours: [6, 18], failing: [], totalItems: 12, ...overrides,
});

function baseClient(overrides = {}) {
  return testClient({
    listFeedSources: vi.fn().mockResolvedValue([source({ id: 'sspai', label: '少数派' })]),
    listFeedItems: vi.fn().mockResolvedValue([news('1')]),
    contentStatus: vi.fn().mockResolvedValue(status()),
    contentGithubTokenStatus: vi.fn().mockResolvedValue(false),
    refreshContent: vi.fn().mockResolvedValue({
      attempted: ['sspai'], succeeded: ['sspai'], notModified: [], failed: [], skipped: [], newItems: 2, nextFetchAt: NOW + 3600,
    } satisfies RefreshReport),
    ...overrides,
  });
}

it('状态行永远说清「上次更新」和「下次更新」', async () => {
  renderWithToasts(<ContentPage client={baseClient()} />);

  const line = await screen.findByText(/上次更新/);
  expect(line).toBeInTheDocument();
  expect(line.textContent).toMatch(/下次自动更新/);
  expect(line.textContent).toMatch(/本地存了 12 条/);
  expect(screen.getByRole('button', { name: '立即刷新' })).toBeInTheDocument();
});

it('未来的时间说「后」，过去的时间说「前」', async () => {
  // 用户截图里那句是「下次自动更新 5 分钟前」：相对时间函数把入参夹成非负、又一律按负数
  // 格式化，于是未来的时间被念成过去的。夹具里 now 是固定值，两半各断言一次。
  const client = baseClient({ contentStatus: vi.fn().mockResolvedValue(status({ lastOkAt: NOW - 600, nextFetchAt: NOW + 1800 })) });
  renderWithToasts(<ContentPage client={client} />);

  const line = await screen.findByText(/上次更新/);
  expect(line.textContent).toMatch(/上次更新 10 ?分钟前/);
  expect(line.textContent).toMatch(/下次自动更新 30 ?分钟后/);
  expect(line.textContent).not.toMatch(/下次自动更新 30 ?分钟前/);
});

it('从没成功过时不写成「已更新」', async () => {
  const client = baseClient({ contentStatus: vi.fn().mockResolvedValue(status({ lastOkAt: null, totalItems: 0 })) });
  renderWithToasts(<ContentPage client={client} />);
  expect(await screen.findByText(/还没成功抓取过/)).toBeInTheDocument();
});

it('抓取失败保留旧内容，并把失败原因与连续次数摆出来', async () => {
  const client = baseClient({
    contentStatus: vi.fn().mockResolvedValue(status({
      failing: [{ sourceId: 'hn', label: 'Hacker News', message: '连接超时', failStreak: 2 }],
    })),
  });
  renderWithToasts(<ContentPage client={client} />);

  expect(await screen.findByText(/Hacker News 连续失败 2 次：连接超时/)).toBeInTheDocument();
  expect(screen.getByText(/下面显示的是上一次成功的结果/)).toBeInTheDocument();
  // 旧内容必须还在。
  expect(screen.getByText('资讯 1')).toBeInTheDocument();
});

it('刷新报告把「成功 / 未变化 / 失败 / 未轮到」分开说', async () => {
  const user = userEvent.setup();
  const client = baseClient({
    refreshContent: vi.fn().mockResolvedValue({
      attempted: ['a', 'b', 'c'], succeeded: ['a'], notModified: ['b'],
      failed: [{ sourceId: 'c', label: 'C', message: '超时', failStreak: 1 }],
      skipped: ['d'], newItems: 3, nextFetchAt: NOW + 3600,
    } satisfies RefreshReport),
  });
  renderWithToasts(<ContentPage client={client} />);

  await user.click(await screen.findByRole('button', { name: '立即刷新' }));

  expect(await screen.findByText('本次抓取')).toBeInTheDocument();
  expect(screen.getByText(/成功 1 · 未变化 1 · 失败 1 · 未轮到 1 · 新增 3 条/)).toBeInTheDocument();
  expect(screen.getByText(/时间预算用完/)).toBeInTheDocument();
});

it('内容为空时给可执行的下一步，并说明更新只在应用运行时发生', async () => {
  const client = baseClient({
    listFeedItems: vi.fn().mockResolvedValue([]),
    contentStatus: vi.fn().mockResolvedValue(status({ lastOkAt: null, totalItems: 0 })),
  });
  renderWithToasts(<ContentPage client={client} />);

  expect(await screen.findByText('还没有资讯')).toBeInTheDocument();
  expect(screen.getByText(/只在应用运行时更新/)).toBeInTheDocument();
});

describe('GitHub 热门', () => {
  it('切换时间范围只在本地过滤，不再联网', async () => {
    const user = userEvent.setup();
    const listFeedItems = vi.fn().mockResolvedValue([
      news('repo-a', {
        sourceId: 'github-week', sourceLabel: 'GitHub 本周热门', title: 'owner/a',
        url: 'https://github.com/owner/a', repo: 'owner/a', stars: 120, summary: 'Rust · 说明',
      }),
    ]);
    const client = baseClient({ listFeedItems });
    renderWithToasts(<ContentPage client={client} />);

    await user.click(await screen.findByRole('tab', { name: 'GitHub 热门' }));
    expect(await screen.findByText('owner/a')).toBeInTheDocument();
    expect(screen.getByText('★ 120')).toBeInTheDocument();
    expect(screen.getByText(/数据来自 GitHub 搜索接口/)).toBeInTheDocument();

    // 时间窗口与页签是同一套分段控件，语义是 tab 而不是 button。
    await user.click(screen.getByRole('tab', { name: '今日' }));
    expect(screen.getByText('这个时间范围还没有数据')).toBeInTheDocument();
    // 切窗口是纯本地过滤：只加载过一次。
    expect(listFeedItems).toHaveBeenCalledTimes(1);
  });
});

describe('订阅源', () => {
  it('每个源显示上次成功时间与连续失败次数', async () => {
    const user = userEvent.setup();
    const client = baseClient({
      listFeedSources: vi.fn().mockResolvedValue([
        source({ id: 'sspai', label: '少数派' }),
        source({ id: 'hn', label: 'Hacker News', failStreak: 3, lastError: '连接超时', lastOkAt: null }),
      ]),
    });
    renderWithToasts(<ContentPage client={client} />);

    await user.click(await screen.findByRole('tab', { name: '订阅源' }));

    expect(await screen.findByText('少数派')).toBeInTheDocument();
    expect(screen.getByText(/连续失败 3 次/)).toBeInTheDocument();
    expect(screen.getByText(/连接超时/)).toBeInTheDocument();
    // 这一行的几个片段在同一条 meta 文本里，用正则匹配整段。
    expect(screen.getByText(/从未成功/)).toBeInTheDocument();
    expect(screen.getByText(/抓取只在应用运行时发生/)).toBeInTheDocument();
  });

  it('停用订阅源走保存而不是另一个开关接口', async () => {
    const user = userEvent.setup();
    const saveFeedSource = vi.fn().mockResolvedValue(source({ id: 'sspai' }));
    const client = baseClient({ saveFeedSource });
    renderWithToasts(<ContentPage client={client} />);

    await user.click(await screen.findByRole('tab', { name: '订阅源' }));
    await user.click(await screen.findByLabelText('启用或停用 少数派'));

    await waitFor(() => expect(saveFeedSource).toHaveBeenCalledWith(expect.objectContaining({ id: 'sspai', enabled: false })));
  });

  it('只刷一个源时带上它的 id', async () => {
    const user = userEvent.setup();
    const refreshContent = vi.fn().mockResolvedValue({
      attempted: ['hn'], succeeded: ['hn'], notModified: [], failed: [], skipped: [], newItems: 0, nextFetchAt: NOW + 3600,
    } satisfies RefreshReport);
    const client = baseClient({
      listFeedSources: vi.fn().mockResolvedValue([source({ id: 'hn', label: 'Hacker News' })]),
      refreshContent,
    });
    renderWithToasts(<ContentPage client={client} />);

    await user.click(await screen.findByRole('tab', { name: '订阅源' }));
    const row = (await screen.findByText('Hacker News')).closest('li')!;
    await user.click(within(row).getByRole('button', { name: '只刷这个' }));

    await waitFor(() => expect(refreshContent).toHaveBeenCalledWith({ sourceId: 'hn', force: true }));
  });

  it('新增订阅源时把类型与地址一起提交，并说明 GitHub 搜索源不是网址', async () => {
    const user = userEvent.setup();
    const saveFeedSource = vi.fn().mockResolvedValue(source({ id: 'new' }));
    const client = baseClient({ saveFeedSource });
    renderWithToasts(<ContentPage client={client} />);

    await user.click(await screen.findByRole('tab', { name: '订阅源' }));
    expect(screen.getByText(/不是网址，而是时间窗口/)).toBeInTheDocument();

    await user.type(screen.getByLabelText('名称'), '我的源');
    await user.type(screen.getByLabelText('地址'), 'https://example.com/feed');
    await user.click(screen.getByRole('button', { name: '添加' }));

    await waitFor(() => expect(saveFeedSource).toHaveBeenCalledWith(expect.objectContaining({
      kind: 'rss', url: 'https://example.com/feed', label: '我的源', enabled: true,
    })));
  });
});

describe('GitHub 令牌', () => {
  it('不填也要能用，并且把差别说清楚', async () => {
    const user = userEvent.setup();
    renderWithToasts(<ContentPage client={baseClient()} />);
    await user.click(await screen.findByRole('tab', { name: '订阅源' }));

    expect(await screen.findByText('未配置')).toBeInTheDocument();
    expect(screen.getByText(/不填也能用/)).toBeInTheDocument();
    expect(screen.getByText(/只存进系统凭据库/)).toBeInTheDocument();
  });

  it('点按钮打开弹窗，填入令牌后保存并提示成功', async () => {
    const user = userEvent.setup();
    const setContentGithubToken = vi.fn().mockResolvedValue(true);
    renderWithToasts(<ContentPage client={baseClient({ setContentGithubToken })} />);
    await user.click(await screen.findByRole('tab', { name: '订阅源' }));

    // 入口不再是 window.prompt：真机上它没有界面，点了等于没点。
    await user.click(await screen.findByRole('button', { name: '填写令牌' }));

    const dialog = await screen.findByRole('dialog');
    await user.type(within(dialog).getByLabelText('GitHub 令牌'), 'ghp_secret');
    await user.click(within(dialog).getByRole('button', { name: '保存' }));

    await waitFor(() => expect(setContentGithubToken).toHaveBeenCalledWith('ghp_secret'));
    expect(await screen.findByText('令牌已存入系统凭据库')).toBeInTheDocument();
    // 成功后弹窗关闭，卡片上的徽章改口。
    await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument());
    expect(await screen.findByText('已配置')).toBeInTheDocument();
  });

  it('已配置时弹窗底栏给出清除路径，清除后徽章回到未配置', async () => {
    const user = userEvent.setup();
    const setContentGithubToken = vi.fn().mockResolvedValue(false);
    const client = baseClient({
      setContentGithubToken,
      contentGithubTokenStatus: vi.fn().mockResolvedValue(true),
    });
    renderWithToasts(<ContentPage client={client} />);
    await user.click(await screen.findByRole('tab', { name: '订阅源' }));

    await user.click(await screen.findByRole('button', { name: '更新令牌' }));
    const dialog = await screen.findByRole('dialog');
    await user.click(within(dialog).getByRole('button', { name: '清除令牌' }));

    await waitFor(() => expect(setContentGithubToken).toHaveBeenCalledWith(null));
    expect(await screen.findByText('令牌已清除')).toBeInTheDocument();
    expect(await screen.findByText('未配置')).toBeInTheDocument();
  });

  it('保存失败时弹窗留在原地并如实提示', async () => {
    const user = userEvent.setup();
    const setContentGithubToken = vi.fn().mockRejectedValue(new Error('凭据库已锁定'));
    renderWithToasts(<ContentPage client={baseClient({ setContentGithubToken })} />);
    await user.click(await screen.findByRole('tab', { name: '订阅源' }));

    await user.click(await screen.findByRole('button', { name: '填写令牌' }));
    const dialog = await screen.findByRole('dialog');
    await user.type(within(dialog).getByLabelText('GitHub 令牌'), 'ghp_secret');
    await user.click(within(dialog).getByRole('button', { name: '保存' }));

    await waitFor(() => expect(setContentGithubToken).toHaveBeenCalled());
    // 失败不能吞掉：弹窗不关，人还在，错误也有提示。
    expect(screen.getByRole('dialog')).toBeInTheDocument();
    expect(await screen.findByText('操作失败，请重试或查看诊断。')).toBeInTheDocument();
  });
});
