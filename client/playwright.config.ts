import { defineConfig } from '@playwright/test';

/// 端到端测试驱动真实二进制。每个测试独享脚本化模型服务、agent 与临时目录。
export default defineConfig({
  testDir: './e2e',
  fullyParallel: true,
  forbidOnly: process.env.CI !== undefined && process.env.CI !== '',
  retries: 0,
  workers: process.env.CI ? 2 : 4,
  reporter: [['list']],
  timeout: 90_000,
  expect: { timeout: 20_000 },
  use: {
    browserName: 'chromium',
    viewport: { width: 1440, height: 960 },
    trace: 'retain-on-failure',
    screenshot: 'only-on-failure',
  },
});
