import react from '@vitejs/plugin-react';
import { configDefaults, defineConfig } from 'vitest/config';

export default defineConfig({
  plugins: [react()],
  server: {
    proxy: {
      '/api': 'http://127.0.0.1:8081',
    },
  },
  test: {
    exclude: [...configDefaults.exclude, 'storybook-static/**', 'e2e/**'],
  },
});
