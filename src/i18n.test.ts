import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join } from 'node:path';

import { describe, expect, it } from 'vitest';

import { en } from '@/locales/en';
import { zhCN } from '@/locales/zh-CN';
import { t, resolveLocale } from '@/i18n';

/**
 * 文案层的不变量。
 *
 * 这一组断言守的是**键集合**，不是某个页面的措辞：
 * - 两种语言的键必须一一对应，缺一条就在这里失败，而不是在界面上悄悄回退成中文；
 * - 代码里用到的键必须存在——包括键表里以字符串形式存放的（例如失效建议表）；
 * - 字典里不能有没人用的键，否则无法判断哪些文案还在生效；
 * - Rust 核心通过 `messageKey` 动态查表的那批键必须存在，静态扫描看不到它们。
 */

const SEGMENT = String.raw`(?:\.[a-zA-Z][a-zA-Z0-9]*|\.[0-9]+)+`;
/** 只有这些前缀是文案键；`api.example.com` 这类数据串不会被误收。 */
const SOURCE_PREFIXES = new Set([
  'action', 'advice', 'app', 'auth', 'capability', 'codex', 'common', 'compat', 'copy', 'credential', 'diag',
  'editor', 'effect', 'empty', 'error', 'group', 'host', 'key', 'logs', 'models', 'nav', 'onboarding',
  'overview', 'page', 'probe', 'probeState', 'providers', 'reason', 'settings', 'shell', 'stage', 'time', 'warning',
  // 扩展与内容三页（工具管理 / 插件中心 / 内容中心）。
  'content', 'plugins', 'tools',
  // 用量页（只读本机 Codex 会话记录）。
  'usage',
  // 应用内更新：侧栏左上角的更新按钮与更新弹窗。
  'update',
]);
/** Rust 侧只会以 messageKey 形式给出这些前缀的键。 */
const CORE_PREFIXES = new Set(['action', 'capability', 'compat', 'credential', 'error', 'group', 'host', 'instance', 'probe', 'reason', 'stage', 'warning']);
// 扩展板块的错误前缀同样以 `error.` 开头，所以不需要单独列出。

/**
 * 动态键的取值来自 TypeScript 类型或 Rust 枚举，静态扫描看不到，因此显式列出。
 * 新增枚举成员时这里必须同步——否则界面会出现一个键名。
 */
const DYNAMIC_KEYS = new Set([
  ...['overview', 'providers', 'codexConfig', 'diagnostics', 'logs', 'settings'].map(name => `nav.${name}`),
  ...['unverified', 'experimental', 'stable', 'unsupported'].map(name => `compat.${name}`),
  ...['route', 'catalog', 'policy', 'restore', 'requiresReload', 'other'].map(name => `group.${name}`),
  ...['draft', 'validating', 'blocked', 'prepared', 'committing', 'awaitingReload', 'verified', 'pending',
    'rollingBack', 'restored', 'conflict', 'failed', 'connect', 'credential', 'model', 'generate'].map(name => `stage.${name}`),
  ...['passed', 'failed', 'skipped', 'running'].map(name => `probeState.${name}`),
  ...['defaultModel', 'providerRoute', 'catalog', 'gatewayProvider', 'contextOverride', 'reasoningDefault',
    'restore', 'test', 'other'].map(name => `reason.${name}`),
  // 扩展板块：导航来自 Page 联合类型，状态/分类/来源来自 Rust 枚举，静态扫描都看不到。
  ...['tools', 'plugins', 'content', 'usage'].map(name => `nav.${name}`),
  ...['ready', 'needsLogin', 'installed', 'unverified', 'notInstalled', 'unsupportedPlatform'].map(name => `tools.status.${name}`),
  ...['cliCode', 'utility', 'runtime'].map(name => `tools.category.${name}`),
  ...['path', 'candidate'].map(name => `tools.pathSource.${name}`),
  // 状态词典按 Rust 的 ToolStatus 枚举拼键名，静态扫描看不到。
  ...['ready', 'needsLogin', 'installed', 'unverified', 'notInstalled', 'unsupportedPlatform']
    .map(name => `tools.legend.${name}`),
]);

function walk(dir: string, match: (file: string) => boolean): string[] {
  return readdirSync(dir).flatMap(entry => {
    const path = join(dir, entry);
    if (statSync(path).isDirectory()) return walk(path, match);
    return match(path) ? [path] : [];
  });
}

function collect(dir: string, files: (file: string) => boolean, pattern: RegExp, prefixes: Set<string>): Set<string> {
  const found = new Set<string>();
  for (const file of walk(dir, files)) {
    if (file.includes('/locales/') || file.includes('.test.')) continue;
    for (const match of readFileSync(file, 'utf8').matchAll(pattern)) {
      const key = match[1]!;
      if (prefixes.has(key.split('.')[0]!)) found.add(key);
    }
  }
  return found;
}

const fromSource = collect('src', file => /\.tsx?$/.test(file), new RegExp(`'([a-z][a-zA-Z0-9]*${SEGMENT})'`, 'g'), SOURCE_PREFIXES);
const fromCore = collect('crates', file => file.endsWith('.rs'), new RegExp(`"([a-z][a-zA-Z0-9]*${SEGMENT})"`, 'g'), CORE_PREFIXES);
// 桌面壳也是 Rust，也会抛带 messageKey 的 CoreError（例如「网关没起来就不能写配置」）。
// 只扫 crates 的话，壳里独有的键会绕过这条守卫：文案缺失，界面直接显示键名。
const fromDesktop = collect('src-tauri', file => file.endsWith('.rs'), new RegExp(`"([a-z][a-zA-Z0-9]*${SEGMENT})"`, 'g'), CORE_PREFIXES);
const required = new Set([...fromSource, ...fromCore, ...fromDesktop, ...DYNAMIC_KEYS]);

function placeholders(value: string): string[] {
  return [...value.matchAll(/\{(\w+)\}/g)].map(match => match[1]!).sort();
}

describe('文案键集合', () => {
  it('两种语言的键完全一致', () => {
    expect(Object.keys(en).sort()).toEqual(Object.keys(zhCN).sort());
  });

  it('代码里用到的键都有文案，包括键表里的字符串键', () => {
    const missing = [...required].filter(key => !(key in zhCN)).sort();
    expect(missing).toEqual([]);
  });

  it('Rust 核心可能抛出的 messageKey 都有文案', () => {
    const missing = [...fromCore].filter(key => !(key in zhCN)).sort();
    expect(missing).toEqual([]);
  });

  it('字典里没有没人用的键', () => {
    const unused = Object.keys(zhCN).filter(key => !required.has(key)).sort();
    expect(unused).toEqual([]);
  });

  it('两种语言的占位符一致，且没有空文案', () => {
    const mismatched = Object.keys(zhCN)
      .filter(key => placeholders(zhCN[key]!) .join() !== placeholders(en[key]!).join())
      .sort();
    expect(mismatched).toEqual([]);
    const empty = Object.keys(zhCN).filter(key => !zhCN[key]!.trim() || !en[key]!.trim());
    expect(empty).toEqual([]);
  });
});

describe('取文案', () => {
  it('替换 {name} 占位符，缺键时返回键本身而不是另一种语言', () => {
    expect(t('time.minutesAgo', { count: 3 })).toBe('3 分钟前');
    // 缺键暴露成键名：中英混排比一个显眼的键名更难发现。
    expect(t('nope.missing')).toBe('nope.missing');
  });

  it('system 偏好按系统语言解析', () => {
    expect(resolveLocale('zh-CN')).toBe('zh-CN');
    expect(resolveLocale('en')).toBe('en');
    expect(['zh-CN', 'en']).toContain(resolveLocale('system'));
  });
});
