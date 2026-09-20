import { render } from '@testing-library/react';
import type { ReactElement } from 'react';

import { ToastHost } from '@/components/Toast';

/**
 * 渲染一个组件并带上提示宿主。
 *
 * 一次性反馈（已保存 / 已清空 / 批量完成）都走全局 Toast，而宿主挂在应用壳里。
 * 组件级用例直接渲染单个组件时，提示会推进队列却没有宿主去画——于是断言「提示出现了」
 * 必然失败。这个helper把宿主一起装上，让组件级用例看到的是真实行为。
 */
export function renderWithToasts(ui: ReactElement) {
  return render(<>{ui}<ToastHost /></>);
}
