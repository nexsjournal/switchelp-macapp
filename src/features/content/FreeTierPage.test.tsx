import { screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';

import { FreeTierPage } from './FreeTierPage';
import { FREE_TIER_CATALOG } from './freeTierData';
import { FREE_TIER_CACHE_KEY } from './freeTierRemote';
import type { FreeTierCatalog } from './freeTierPolicy';
import { testClient } from '../../../tests/helpers/client';
import { renderWithToasts } from '../../../tests/helpers/render';

/**
 * 免费额度页（docs/design/09）。
 *
 * 数据默认随包静态清单，在线刷新（freeTierRemote.ts）让核实能在两次发版之间
 * 先行。断言的对象是：条目卡片、外链守卫、「在网关中接入」联动、退役条目的
 * 展示边界，以及在线刷新的版本门与提示（自动检查安静、手动检查有交代）。
 */
function renderPage(overrides = {}) {
  const openExternalUrl = vi.fn().mockResolvedValue(undefined);
  const onAccessInGateway = vi.fn();
  renderWithToasts(<FreeTierPage client={testClient({ openExternalUrl, ...overrides })}
    onAccessInGateway={onAccessInGateway} />);
  return { openExternalUrl, onAccessInGateway };
}

/** 造一版「远端更新」：抬 version、换核实日期、带一条随包没有的条目。 */
function remoteCatalog(): FreeTierCatalog {
  return {
    version: FREE_TIER_CATALOG.version + 1,
    verifiedAt: '2026-10-06',
    entries: [{
      id: 'example-cloud', provider: 'ExampleCloud', category: 'model_free_tier',
      title: 'v4 独有条目：每天 100 次免费调用', quota: '注册即送，无需绑卡',
      docsUrl: 'https://example.com/docs', claimUrl: 'https://example.com/claim',
      claimFlow: 'instant', lastVerifiedAt: '2026-10-06',
    }],
  };
}

describe('免费额度页', () => {
  it('列出官方条目；去领取与官方说明都走 openExternalUrl 守卫', async () => {
    const { openExternalUrl } = renderPage();
    const user = userEvent.setup();

    // 用各卡唯一的标题文案定位（厂商名同时出现在 SVG 的 <title> 里，不能按名字查）。
    const openrouterCard = await screen.findByText(':free 模型变体每日免费调用').then(el => el.closest('article')!);
    await user.click(within(openrouterCard).getByRole('button', { name: '去领取' }));
    expect(openExternalUrl).toHaveBeenCalledWith('https://openrouter.ai/keys');
    await user.click(within(openrouterCard).getByRole('button', { name: '官方说明' }));
    expect(openExternalUrl).toHaveBeenCalledWith('https://openrouter.ai/docs/api-reference/limits');
  });

  it('有预设的条目给「在网关中接入」，并把预设 id 交给宿主', async () => {
    const { onAccessInGateway } = renderPage();
    const user = userEvent.setup();
    const card = await screen.findByText(':free 模型变体每日免费调用').then(el => el.closest('article')!);
    await user.click(within(card).getByRole('button', { name: '在网关中接入' }));
    expect(onAccessInGateway).toHaveBeenCalledWith('openrouter');
  });

  it('退役条目置灰保留：没有去领取、没有网关入口，只留官方说明', async () => {
    renderPage();
    const user = userEvent.setup();
    const retiredCard = await screen.findByText(/GitHub Models 白嫖/).then(el => el.closest('article')!);
    expect(within(retiredCard).getByText('已退役')).toBeInTheDocument();
    expect(within(retiredCard).queryByRole('button', { name: '去领取' })).toBeNull();
    expect(within(retiredCard).queryByRole('button', { name: '在网关中接入' })).toBeNull();
    await user.click(within(retiredCard).getByRole('button', { name: '官方说明' }));
  });

  it('「没有免费档」的反例条目只有官方说明，不提供领取', async () => {
    renderPage();
    const deepseekCard = await screen.findByText(/均非官方/).then(el => el.closest('article')!);
    expect(within(deepseekCard).queryByRole('button', { name: '去领取' })).toBeNull();
    expect(within(deepseekCard).getByRole('button', { name: '官方说明' })).toBeInTheDocument();
  });

  it('类别筛选：切到「试用金」后只剩该类条目', async () => {
    renderPage();
    const user = userEvent.setup();
    expect(await screen.findByText(':free 模型变体每日免费调用')).toBeInTheDocument();
    await user.click(screen.getByRole('tab', { name: '试用金 / 新客额度' }));
    expect(screen.queryByText(':free 模型变体每日免费调用')).not.toBeInTheDocument();
    expect(screen.getByText('每款豆包模型赠免费 tokens')).toBeInTheDocument();
  });
});

describe('免费额度页 · 在线刷新', () => {
  beforeEach(() => localStorage.clear());

  it('打开页面自动检查一次；远端与随包同版本时不打扰', async () => {
    const fetchFreeTierCatalog = vi.fn().mockResolvedValue(JSON.stringify(FREE_TIER_CATALOG));
    renderPage({ fetchFreeTierCatalog });
    await waitFor(() => expect(fetchFreeTierCatalog).toHaveBeenCalledTimes(1));
    expect(screen.getByText('清单 v3 · 核实 2026-09-30 · 随应用更新')).toBeInTheDocument();
    expect(screen.queryByText(/已更新到/)).toBeNull();
  });

  it('缓存里有采纳过的更新清单：直接上路，且 24 小时内不再自动请求', async () => {
    localStorage.setItem(FREE_TIER_CACHE_KEY, JSON.stringify({ attemptedAt: Date.now(), catalog: remoteCatalog() }));
    const fetchFreeTierCatalog = vi.fn().mockResolvedValue(JSON.stringify(FREE_TIER_CATALOG));
    renderPage({ fetchFreeTierCatalog });
    expect(await screen.findByText('清单 v4 · 核实 2026-10-06 · 已在线同步')).toBeInTheDocument();
    expect(screen.getByText('v4 独有条目：每天 100 次免费调用')).toBeInTheDocument();
    expect(fetchFreeTierCatalog).not.toHaveBeenCalled();
  });

  it('手动检查拉到新版本：页头与卡片换新、写缓存、出一条提示', async () => {
    // 缓存里只记账、没采纳过清单 → 自动检查歇着，留给手动检查测。
    localStorage.setItem(FREE_TIER_CACHE_KEY, JSON.stringify({ attemptedAt: Date.now(), catalog: null }));
    const fetchFreeTierCatalog = vi.fn().mockResolvedValue(JSON.stringify(remoteCatalog()));
    renderPage({ fetchFreeTierCatalog });
    expect(screen.getByText('清单 v3 · 核实 2026-09-30 · 随应用更新')).toBeInTheDocument();
    const user = userEvent.setup();
    await user.click(screen.getByRole('button', { name: '检查更新' }));
    expect(await screen.findByText('免费额度清单已更新到 v4（核实 2026-10-06）')).toBeInTheDocument();
    expect(screen.getByText('清单 v4 · 核实 2026-10-06 · 已在线同步')).toBeInTheDocument();
    expect(screen.getByText('v4 独有条目：每天 100 次免费调用')).toBeInTheDocument();
    const cached = JSON.parse(localStorage.getItem(FREE_TIER_CACHE_KEY)!);
    expect(cached.catalog.version).toBe(FREE_TIER_CATALOG.version + 1);
  });

  it('手动检查失败（命令报错）：提示原文，清单不动', async () => {
    localStorage.setItem(FREE_TIER_CACHE_KEY, JSON.stringify({ attemptedAt: Date.now(), catalog: null }));
    const fetchFreeTierCatalog = vi.fn().mockRejectedValue({
      code: 'INTERNAL', messageKey: 'error.freeTierRemote', safeDetails: ['网络断了'],
      retryable: false, recoveryActions: [],
    });
    renderPage({ fetchFreeTierCatalog });
    const user = userEvent.setup();
    await user.click(screen.getByRole('button', { name: '检查更新' }));
    expect(await screen.findByText('网络断了')).toBeInTheDocument();
    expect(screen.getByText('清单 v3 · 核实 2026-09-30 · 随应用更新')).toBeInTheDocument();
  });

  it('远端数据没过校验：按失败提示，不渲染来路不明的条目', async () => {
    localStorage.setItem(FREE_TIER_CACHE_KEY, JSON.stringify({ attemptedAt: Date.now(), catalog: null }));
    const fetchFreeTierCatalog = vi.fn().mockResolvedValue('{"version":4,"junk":true}');
    renderPage({ fetchFreeTierCatalog });
    const user = userEvent.setup();
    await user.click(screen.getByRole('button', { name: '检查更新' }));
    expect(await screen.findByText('在线清单获取失败，沿用当前版本')).toBeInTheDocument();
    expect(screen.queryByText(/v4 独有条目/)).toBeNull();
    expect(screen.getByText('清单 v3 · 核实 2026-09-30 · 随应用更新')).toBeInTheDocument();
  });
});
