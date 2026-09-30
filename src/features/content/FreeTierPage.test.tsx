import { screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';

import { FreeTierPage } from './FreeTierPage';
import { testClient } from '../../../tests/helpers/client';
import { renderWithToasts } from '../../../tests/helpers/render';

/**
 * 免费额度页（docs/design/09）。
 *
 * 数据是随包静态清单（不走内容中心的抓取调度），断言的对象是：
 * 条目卡片、外链守卫、「在网关中接入」联动与退役条目的展示边界。
 */
function renderPage(overrides = {}) {
  const openExternalUrl = vi.fn().mockResolvedValue(undefined);
  const onAccessInGateway = vi.fn();
  renderWithToasts(<FreeTierPage client={testClient({ openExternalUrl, ...overrides })}
    onAccessInGateway={onAccessInGateway} />);
  return { openExternalUrl, onAccessInGateway };
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
