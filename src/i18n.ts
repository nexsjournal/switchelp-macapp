/**
 * 文案运行时。
 *
 * 三件事：
 * - `t(key, vars)` 取文案，`{name}` 形式的占位符会被替换；
 * - 语言偏好三档（`zh-CN` / `en` / `system`），`system` 解析成具体语言后写入
 *   `<html lang>`，CSS 与读屏都按最终值工作；
 * - 键缺失时返回键本身并把问题暴露出来，**不回退到另一种语言**：
 *   中英混排比一个显眼的键名更难发现。
 *
 * 偏好存 localStorage：语言和主题一样是界面偏好，不是秘密，也不参与配置事务。
 */

import { useSyncExternalStore } from 'react';

import { zhCN } from '@/locales/zh-CN';
import { en } from '@/locales/en';

export type LocalePreference = 'zh-CN' | 'en' | 'system';
export type ResolvedLocale = 'zh-CN' | 'en';

const STORAGE_KEY = 'gptswitch.locale';
const DICTIONARIES: Record<ResolvedLocale, Record<string, string>> = { 'zh-CN': zhCN, en };

let current: ResolvedLocale = 'zh-CN';

/**
 * 语言变更的订阅者。
 *
 * `t()` 读的是模块级状态，React 不知道它变了；订阅让 `useLocale()` 能触发重渲染，
 * 也让缓存了 `t()` 结果的 `useMemo` 有依赖可挂。
 */
const listeners = new Set<() => void>();

function notify(): void {
  for (const listener of listeners) listener();
}

/** 系统语言是否属于英文环境。 */
function systemPrefersEnglish(): boolean {
  const tag = typeof navigator !== 'undefined' ? navigator.language ?? '' : '';
  return tag.toLowerCase().startsWith('en');
}

export function resolveLocale(preference: LocalePreference): ResolvedLocale {
  if (preference === 'system') return systemPrefersEnglish() ? 'en' : 'zh-CN';
  return preference;
}

export function readLocalePreference(): LocalePreference {
  try {
    const stored = localStorage.getItem(STORAGE_KEY);
    if (stored === 'zh-CN' || stored === 'en' || stored === 'system') return stored;
  } catch {
    // 存储不可用（隐私模式等）时回落到默认语言，不影响渲染。
  }
  // 首次启动跟随系统，比强行用中文更友好。
  return 'system';
}

export function applyLocale(preference: LocalePreference): ResolvedLocale {
  const next = resolveLocale(preference);
  const changed = next !== current;
  current = next;
  if (typeof document !== 'undefined') {
    document.documentElement.lang = current;
  }
  if (changed) notify();
  return current;
}

export function setLocalePreference(preference: LocalePreference): ResolvedLocale {
  try {
    localStorage.setItem(STORAGE_KEY, preference);
  } catch {
    // 写不进去只影响“下次还记得”，当前语言照常生效。
  }
  return applyLocale(preference);
}

export function currentLocale(): ResolvedLocale {
  return current;
}

/** 语言在偏好为 system 时跟随系统变化。 */
export function watchSystemLocale(onChange: (locale: ResolvedLocale) => void): () => void {
  if (typeof window === 'undefined' || typeof window.addEventListener !== 'function') return () => {};
  const handler = () => {
    // `applyLocale` 自己会通知订阅者，所以这里默认回调可以是空的。
    if (readLocalePreference() === 'system') onChange(applyLocale('system'));
  };
  window.addEventListener('languagechange', handler);
  return () => window.removeEventListener('languagechange', handler);
}

/** 取文案并替换 `{name}` 占位符。缺键时返回键本身，便于一眼看出漏翻。 */
export function t(key: string, vars?: Record<string, string | number>): string {
  return translate(current, key, vars);
}

function translate(locale: ResolvedLocale, key: string, vars?: Record<string, string | number>): string {
  const template = DICTIONARIES[locale][key];
  if (template === undefined) return key;
  if (!vars) return template;
  return template.replace(/\{(\w+)\}/g, (match, name: string) => {
    const value = vars[name];
    return value === undefined ? match : String(value);
  });
}

/** 订阅当前语言；语言变化时组件会重渲染。 */
export function useLocale(): ResolvedLocale {
  return useSyncExternalStore(subscribeLocale, currentLocale);
}

export function subscribeLocale(listener: () => void): () => void {
  listeners.add(listener);
  return () => { listeners.delete(listener); };
}
