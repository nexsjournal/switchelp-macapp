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
