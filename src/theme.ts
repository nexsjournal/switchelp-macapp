import { isTauri } from '@tauri-apps/api/core';
import { getCurrentWindow } from '@tauri-apps/api/window';

/**
 * 主题控制。
 *
 * 三档偏好：`dark` / `light` / `system`。`system` 会解析成具体的 dark 或 light 再写到
 * `data-theme` 上——CSS 只认最终值，不在样式里写媒体查询分支，避免两套规则各自漂移。
 *
 * 存储位置是 localStorage：主题是界面偏好，不是秘密，也不是需要参与配置事务的数据。
 * 密钥类信息仍然只走系统凭据库，这一点没有变化（见 docs/architecture/05-security-and-platforms.md）。
 */

export type ThemePreference = 'dark' | 'light' | 'system';
export type ResolvedTheme = 'dark' | 'light';

const STORAGE_KEY = 'gptswitch.theme';

/** 系统是否偏好亮色。 */
function systemPrefersLight(): boolean {
  return typeof window !== 'undefined'
    && typeof window.matchMedia === 'function'
    && window.matchMedia('(prefers-color-scheme: light)').matches;
}

export function resolveTheme(preference: ThemePreference): ResolvedTheme {
  if (preference === 'system') return systemPrefersLight() ? 'light' : 'dark';
  return preference;
}

export function readThemePreference(): ThemePreference {
  try {
    const stored = localStorage.getItem(STORAGE_KEY);
    if (stored === 'dark' || stored === 'light' || stored === 'system') return stored;
  } catch {
    // 存储不可用（隐私模式等）时回落到默认，不影响渲染。
  }
  return 'dark';
}

/** 把解析后的主题写到根元素。返回实际生效的主题。 */
export function applyTheme(preference: ThemePreference): ResolvedTheme {
  const resolved = resolveTheme(preference);
  document.documentElement.dataset.theme = resolved;
  // 让原生控件（select、滚动条、复选框）跟随主题。
  document.documentElement.style.colorScheme = resolved;
  applyWindowTheme(resolved);
  return resolved;
}

/**
 * 让原生窗口外观跟随应用主题。
 *
 * macOS 的标题栏是透明的（`titleBarStyle: Overlay`），交通灯按钮由系统按**窗口外观**取色；
 * 只改 DOM 的话，系统是浅色而应用选了深色主题时，窗口顶部会露出一圈不协调的浅色。
 */
function applyWindowTheme(resolved: ResolvedTheme): void {
  if (!isTauri()) return;
  void getCurrentWindow()
    .setTheme(resolved)
    // 平台不支持或缺少权限时忽略：界面主题本身照常生效。
    .catch(() => {});
}

export function setThemePreference(preference: ThemePreference): ResolvedTheme {
  try {
    localStorage.setItem(STORAGE_KEY, preference);
  } catch {
    // 写不进去只影响“下次还记得”，当前主题照常生效。
  }
  return applyTheme(preference);
}

/** 跟随系统的偏好变化：只在偏好为 system 时重新解析。 */
export function watchSystemTheme(onChange: (resolved: ResolvedTheme) => void): () => void {
  if (typeof window === 'undefined' || typeof window.matchMedia !== 'function') return () => {};
  const query = window.matchMedia('(prefers-color-scheme: light)');
  const handler = () => {
    if (readThemePreference() === 'system') onChange(applyTheme('system'));
  };
  query.addEventListener('change', handler);
  return () => query.removeEventListener('change', handler);
}
