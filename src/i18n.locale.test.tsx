import { render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it } from 'vitest';

import { App } from '@/app/App';
import { currentLocale, readLocalePreference } from '@/i18n';
import { provider, testClient } from '../tests/helpers/client';

/**
 * 语言切换的行为。
 *
 * 文案取自模块级状态，React 看不到它的变化；这一组用例锁住的是**整棵树都会重渲染**：
 * 侧栏、页标题、设置页都跟着换语言，而不是只有触发切换的那个控件。
 */

const client = () => testClient({ listProviders: async () => ({ items: [provider], nextCursor: null }) });

/** 键名形状的片段一旦出现在界面上，就说明某个键漏了文案。 */
const KEY_SHAPED = /\b(action|common|compat|copy|credential|diag|empty|error|group|host|nav|probe|probeState|reason|stage|usage|warning)\.[a-zA-Z]/;

function bodyText(): string {
  return document.body.textContent ?? '';
}

describe('语言切换', () => {
  it('切到英文后侧栏、页标题与设置页一起换语言', async () => {
    const user = userEvent.setup();
    render(<App client={client()} />);

    const nav = () => screen.getByRole('navigation');
    await screen.findByText('测试供应商');
    expect(within(nav()).getByRole('button', { name: '概览' })).toBeInTheDocument();
    expect(document.documentElement.lang).toBe('zh-CN');

    // 设置入口按设计放在侧栏底部，不在 <nav> 里。
    await user.click(screen.getByRole('button', { name: '设置' }));
    await user.selectOptions(screen.getByLabelText('语言'), 'en');

    // 侧栏与页标题同时变成英文：只更新设置页会让它们留在旧语言。
    expect(within(nav()).getByRole('button', { name: 'Overview' })).toBeInTheDocument();
    expect(screen.getByRole('heading', { level: 1, name: 'Settings' })).toBeInTheDocument();
    expect(document.documentElement.lang).toBe('en');
    expect(currentLocale()).toBe('en');
    // 选择本身也被记住。
    expect(readLocalePreference()).toBe('en');

    // 切回去同样立即生效。
    await user.selectOptions(screen.getByLabelText('Language'), 'zh-CN');
    expect(within(nav()).getByRole('button', { name: '概览' })).toBeInTheDocument();
    expect(document.documentElement.lang).toBe('zh-CN');
  });

  it('两种语言下都不把文案键摆到界面上', async () => {
    const user = userEvent.setup();
    render(<App client={client()} />);
    await screen.findByText('测试供应商');
    expect(bodyText()).not.toMatch(KEY_SHAPED);

    await user.click(screen.getByRole('button', { name: '设置' }));
    await user.selectOptions(screen.getByLabelText('语言'), 'en');
    expect(bodyText()).not.toMatch(KEY_SHAPED);
  });
});
