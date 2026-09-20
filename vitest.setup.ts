import '@testing-library/jest-dom/vitest';
import { beforeEach } from 'vitest';

import { setLocalePreference } from '@/i18n';
import { resetToasts } from '@/components/Toast';

/**
 * 测试固定用简体中文断言。
 *
 * 产品里语言默认跟随系统，而 jsdom 的 `navigator.language` 是 `en-US`——
 * 不固定的话，断言中文文案的用例会因为跑在英文下而失败。
 * 英文侧由 src/i18n.test.ts 从键集合与渲染结果两头覆盖。
 */
beforeEach(() => {
  // 连偏好一起写死：界面语言与 `readLocalePreference()` 必须一致，否则设置页读到的是 system。
  // 这里不动 localStorage 的其它内容——用例之间本来就靠已写入的界面偏好串起上下文（例如向导是否已看过）。
  setLocalePreference('zh-CN');
  // 提示队列是模块级状态：不清的话上一条用例的提示会被下一条看见（断言「没有提示」时尤其致命）。
  resetToasts();
});
