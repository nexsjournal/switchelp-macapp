import { render, screen } from '@testing-library/react';

import { renderReleaseNotes } from './releaseNotes';

/**
 * 发布说明渲染。
 *
 * 守两件事：认得的语法要真的变成元素（不是原样露 `##`、`-`），认不出的**不能被吞掉**
 * （宁可朴素，也不能让作者写的内容消失）。
 */
test('标题、列表、加粗、行内代码都渲染成元素，而不是原样显示标记', () => {
  const { container } = render(<>{renderReleaseNotes('# 大标题\n\n- 一条**加粗**的改动\n- 带 `code` 的一条')}</>);

  expect(screen.getByText('大标题')).toBeInTheDocument();
  expect(container.querySelector('h4')?.textContent).toBe('大标题');
  expect(screen.getByText('加粗')).toBeInTheDocument();
  expect(screen.getByText('加粗').tagName).toBe('STRONG');
  expect(screen.getByText('code').tagName).toBe('CODE');
  // 标记本身不许出现在文本里。
  expect(container.textContent).not.toContain('##');
  expect(container.textContent).not.toContain('**');
  expect(container.textContent).not.toContain('- 一条');
  expect(container.querySelectorAll('li')).toHaveLength(2);
});

test('多个 # 级别都渲染成同一级小标题：源文件层级不该决定弹窗里的观感', () => {
  const { container } = render(<>{renderReleaseNotes('# 一\n\n## 二\n\n### 三')}</>);
  expect([...container.querySelectorAll('h4')].map(node => node.textContent)).toEqual(['一', '二', '三']);
});

test('认不出的行按普通文字显示，不丢内容', () => {
  const { container } = render(<>{renderReleaseNotes('| 平台 | 文件 |\n| --- | --- |')}</>);
  expect(container.textContent).toContain('| 平台 | 文件 |');
  expect(container.textContent).toContain('| --- | --- |');
  expect(container.querySelectorAll('p')).toHaveLength(2);
});

test('空说明返回空，不留空段落', () => {
  const { container } = render(<>{renderReleaseNotes('\n\n')}</>);
  expect(container.textContent).toBe('');
});
