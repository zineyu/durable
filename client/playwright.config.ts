import { defineConfig } from '@playwright/test';

/// 端到端测试驱动真实二进制。每个测试独享脚本化模型服务、agent 与临时目录。
export default defineConfig({
  testDir: './e2e',
  fullyParallel: true,
  forbidOnly: process.env.CI !== undefined && process.env.CI !== '',
  retries: 0,
  // 每个 worker 自带 agent、executor 与浏览器；4 个并行在开发机上会把工具卡片的 20 秒断言挤超时。
  workers: 2,
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
