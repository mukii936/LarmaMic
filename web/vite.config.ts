import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';

// Tên file cố định (không băm) để app Rust nhúng và phục vụ được bằng đường dẫn cố định.
export default defineConfig({
  plugins: [react()],
  build: {
    target: 'safari14',
    outDir: 'dist',
    assetsDir: 'assets',
    cssCodeSplit: false,
    chunkSizeWarningLimit: 1024,
    rollupOptions: {
      output: {
        entryFileNames: 'assets/app.js',
        chunkFileNames: 'assets/app-[name].js',
        assetFileNames: 'assets/app[extname]',
      },
    },
  },
  server: {
    // `npm run dev`: chuyển tiếp WebSocket tới app LarmaMic đang chạy trên PC
    proxy: { '/ws': { target: 'wss://localhost:8443', ws: true, secure: false } },
  },
});
