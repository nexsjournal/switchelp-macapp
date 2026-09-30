import { deriveAccentRamp, setAccentPreference } from './accent';
import { applyTheme, readThemePreference, resolveTheme, setThemePreference } from './theme';

function mockSystemPrefersLight(light: boolean) {
  window.matchMedia = vi.fn().mockImplementation((query: string) => ({
    matches: light && query.includes('light'),
    media: query,
    addEventListener: vi.fn(),
    removeEventListener: vi.fn(),
  })) as unknown as typeof window.matchMedia;
}

beforeEach(() => { localStorage.clear(); mockSystemPrefersLight(false); });

test('system 会解析成具体主题，而不是把 system 写进 DOM', () => {
  mockSystemPrefersLight(true);
  expect(resolveTheme('system')).toBe('light');
  expect(applyTheme('system')).toBe('light');
  expect(document.documentElement.dataset.theme).toBe('light');

  mockSystemPrefersLight(false);
  expect(applyTheme('system')).toBe('dark');
  expect(document.documentElement.dataset.theme).toBe('dark');
});

test('显式选择不受系统偏好影响', () => {
  mockSystemPrefersLight(true);
  expect(applyTheme('dark')).toBe('dark');
  expect(document.documentElement.dataset.theme).toBe('dark');
});

test('偏好会被记住，下次启动沿用', () => {
  setThemePreference('light');
  expect(readThemePreference()).toBe('light');
  setThemePreference('system');
  expect(readThemePreference()).toBe('system');
});

test('存储不可用时回落到默认，而不是抛错', () => {
  const spy = vi.spyOn(Storage.prototype, 'getItem').mockImplementation(() => { throw new Error('blocked'); });
  expect(readThemePreference()).toBe('dark');
  spy.mockRestore();
});

test('color-scheme 跟随主题，原生控件才不会与页面打架', () => {
  applyTheme('light');
  expect(document.documentElement.style.colorScheme).toBe('light');
  applyTheme('dark');
  expect(document.documentElement.style.colorScheme).toBe('dark');
});

/*
 * 主题与主题色的衔接处：主题色按**当前主题**算自己的档位（浅色主题压白字、
 * 深色主题压深字），所以换主题就得重推一遍，这条链路断了不会崩，只会让链接
 * 或按钮在新主题下发白、看不见。`accent.test.ts` 覆盖推导本身，这里只覆盖接线。
 */

test('切换主题会把自定义主题色按新主题的档位重新推一遍', () => {
  setAccentPreference('#2563eb');
  expect(applyTheme('light')).toBe('light');
  expect(document.documentElement.style.getPropertyValue('--accent')).toBe(deriveAccentRamp('#2563eb', 'light').accent);
  expect(applyTheme('dark')).toBe('dark');
  expect(document.documentElement.style.getPropertyValue('--accent')).toBe(deriveAccentRamp('#2563eb', 'dark').accent);
});

test('存储里没有主题色时，切换主题不会凭空写一个内联覆盖', () => {
  // 先放一个脏覆盖：默认档若只是「不写」而不是「删掉」，上一次自定义留下的内联变量
  // 会盖住 tokens.css——主题切了，颜色还是旧的。
  document.documentElement.style.setProperty('--accent', '#ff0000');
  applyTheme('light');
  expect(document.documentElement.style.getPropertyValue('--accent')).toBe('');
});
