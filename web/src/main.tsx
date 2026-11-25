import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import { App } from './App';
import './styles.css';
import { TOKEN } from './useMic';

// Manifest mang mã ghép nối trong start_url để lối tắt ở Màn hình chính mở là nối được ngay
if (TOKEN) {
  const link = document.createElement('link');
  link.rel = 'manifest';
  link.href = `/manifest.webmanifest?k=${encodeURIComponent(TOKEN)}`;
  document.head.appendChild(link);
}

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
