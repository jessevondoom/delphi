import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';

// The window loads these files from disk (Tauri's `frontendDist`), so paths are relative and nothing is fetched at run time.
export default defineConfig({
  plugins: [react()],
  base: './',
  clearScreen: false,
  server: { port: 5199, strictPort: true },
  build: { outDir: 'dist', emptyOutDir: true, target: 'es2022' },
});
