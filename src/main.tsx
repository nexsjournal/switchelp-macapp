import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';

import { App } from '@/app/App';
import { applyTheme, readThemePreference, watchSystemTheme } from './theme';
import '@/styles/tokens.css';
import '@/styles/global.css';

// 首帧之前就把主题写到根元素：晚一步会先闪一下默认底色。
applyTheme(readThemePreference());
watchSystemTheme(() => {});

const container = document.getElementById('root');
if (!container) {
  throw new Error('缺少 #root 容器');
}

createRoot(container).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
