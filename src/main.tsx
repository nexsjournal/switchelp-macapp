import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';

import { App } from '@/app/App';
import '@/styles/tokens.css';
import '@/styles/global.css';

// 首版默认采用参考截图的深色主题。
document.documentElement.dataset.theme = 'dark';

const container = document.getElementById('root');
if (!container) {
  throw new Error('缺少 #root 容器');
}

createRoot(container).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
