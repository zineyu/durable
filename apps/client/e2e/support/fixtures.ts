import { test as base } from '@playwright/test';

import { Harness } from './harness';

type Fixtures = {
  /// 每个测试独享的完整栈：脚本化模型服务、真实 agent 与临时目录。
  app: Harness;
};

export const test = base.extend<Fixtures>({
  // eslint-disable-next-line no-empty-pattern -- fixture 回调必须写成解构参数。
  app: async ({}, use) => {
    const app = await Harness.start();
    try {
      await use(app);
    } finally {
      await app.dispose();
    }
  },
});

export { expect } from '@playwright/test';
