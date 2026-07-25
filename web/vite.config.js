import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';

// `npm run build` → dist/ (served by the Rust `serve` command).
// `npm run dev` runs Vite on :5173 and proxies /api to the Rust server on :8787.
export default defineConfig({
  plugins: [react()],
  build: { outDir: 'dist' },
  server: { proxy: { '/api': 'http://localhost:8787' } },
});
