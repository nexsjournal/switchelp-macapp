import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';

import { App } from '@/app/App';
import { applyTheme, readThemePreference, watchSystemTheme } from './theme';
import { applyLocale, readLocalePreference, watchSystemLocale } from './i18n';
import '@/styles/tokens.css';
import '@/styles/global.css';

// 首帧之前就把主题写到根元素：晚一步会先闪一下默认底色。
applyTheme(readThemePreference());
watchSystemTheme(() => {});
// 语言同理：解析后的值写进 <html lang>，CSS 与读屏都按最终语言工作。
applyLocale(readLocalePreference());
watchSystemLocale(() => {});

const container = document.getElementById('root');
if (!container) {
  throw new Error('缺少 #root 容器');
}

createRoot(container).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
