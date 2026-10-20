import { mkdir, readFile, rm, stat } from 'node:fs/promises';
import { createServer } from 'node:http';
import { extname, join, normalize, sep } from 'node:path';
import { fileURLToPath } from 'node:url';
import { chromium } from 'playwright';

const root = fileURLToPath(new URL('../storybook-static/', import.meta.url));
const output = fileURLToPath(new URL('../screenshots/', import.meta.url));

const contentTypes = new Map([
  ['.html', 'text/html; charset=utf-8'],
  ['.js', 'text/javascript; charset=utf-8'],
  ['.css', 'text/css; charset=utf-8'],
  ['.json', 'application/json; charset=utf-8'],
  ['.map', 'application/json; charset=utf-8'],
  ['.svg', 'image/svg+xml'],
  ['.png', 'image/png'],
  ['.woff2', 'font/woff2'],
]);

const storyViewports = [
  { label: 'compact', width: 1024, height: 720 },
  { label: 'narrow', width: 768, height: 600 },
];

const server = createServer(async (request, response) => {
  const url = new URL(request.url ?? '/', 'http://127.0.0.1');
  const path = normalize(join(root, decodeURIComponent(url.pathname)));
  if (!path.startsWith(root.endsWith(sep) ? root : root + sep)) {
    response.writeHead(403);
    response.end('forbidden');
    return;
  }

  try {
    const info = await stat(path);
    if (!info.isFile()) {
      response.writeHead(404);
      response.end('not found');
      return;
    }
    const file = await readFile(path);
    response.writeHead(200, {
      'content-type': contentTypes.get(extname(path)) ?? 'application/octet-stream',
    });
    response.end(file);
  } catch (error) {
    if (error instanceof Error && 'code' in error && error.code === 'ENOENT') {
      response.writeHead(404);
      response.end('not found');
      return;
    }
    throw error;
  }
});

await new Promise((resolve, reject) => {
  server.once('error', reject);
  server.listen(0, '127.0.0.1', resolve);
});

const address = server.address();
if (address === null || typeof address === 'string') {
  throw new Error('截图服务器未获得端口');
}

const index = JSON.parse(await readFile(join(root, 'index.json'), 'utf8'));
const stories = Object.values(index.entries);

if (stories.length === 0) {
  throw new Error('storybook-static/index.json 没有任何 story');
}

const isConsolePage = (id) => id.startsWith('pages-console');

const switcherStories = (id) =>
  id.startsWith('pages-console') ||
  id.startsWith('components-sidebar') ||
  id.startsWith('components-nodeswitcher') ||
  id.startsWith('components-appshell');

const emptyNodeStories = new Set([
  'components-nodeswitcher--empty',
  'components-sidebar--no-nodes',
  'pages-console--no-nodes',
]);

const noSelectionStories = new Set([
  'components-nodeswitcher--empty',
  'components-nodeswitcher--unknown-selection',
  'components-sidebar--unknown-node',
  'components-sidebar--no-nodes',
  'pages-console--no-nodes',
  'pages-console--unknown-node',
]);

async function assertNoHorizontalOverflow(page) {
  const overflows = await page.evaluate(
    () => document.documentElement.scrollWidth > document.documentElement.clientWidth,
  );
  if (overflows) {
    throw new Error('页面出现横向溢出');
  }
}

async function assertChromeInViewport(page) {
  const viewport = page.viewportSize();
  if (viewport === null) {
    throw new Error('未取得 viewport 尺寸');
  }

  const composer = page.locator('[class*="composer"]').first();
  await composer.waitFor({ state: 'visible' });
  const composerBox = await composer.boundingBox();
  if (composerBox === null) {
    throw new Error('未取得输入区尺寸');
  }
  if (composerBox.y < 0 || composerBox.y + composerBox.height > viewport.height + 1) {
    throw new Error(
      `输入区不在 viewport 内: ${composerBox.y}..${composerBox.y + composerBox.height} / ${viewport.height}`,
    );
  }

  const queue = page.locator('[class*="strip"]').first();
  if ((await queue.count()) > 0) {
    const queueBox = await queue.boundingBox();
    if (queueBox !== null && (queueBox.y < 0 || queueBox.y + queueBox.height > viewport.height + 1)) {
      throw new Error(
        `队列不在 viewport 内: ${queueBox.y}..${queueBox.y + queueBox.height} / ${viewport.height}`,
      );
    }
  }
}

async function assertColumnAlignment(page) {
  const boxes = await page.evaluate(() => {
    const pick = (element) => {
      if (element === null) return null;
      const rect = element.getBoundingClientRect();
      return { left: rect.left, right: rect.right };
    };
    return {
      composer: pick(document.querySelector('[class*="composer"]')),
      headerTitle: pick(document.querySelector('header h1')),
      message: pick(document.querySelector('[class*="transcript"] article')),
    };
  });
  if (boxes.composer === null) {
    throw new Error('未找到输入区');
  }
  if (boxes.headerTitle !== null && Math.abs(boxes.headerTitle.left - boxes.composer.left) > 1) {
    throw new Error(
      `页头标题与输入区左边缘未对齐: ${boxes.headerTitle.left} vs ${boxes.composer.left}`,
    );
  }
  if (boxes.message !== null) {
    const inset = boxes.message.left - boxes.composer.left;
    if (inset < 15 || inset > 17) {
      throw new Error(`消息列相对输入区内缩不是 16px: ${inset}`);
    }
  }
}

async function assertBranchSwitcherPlacement(page) {
  const switchers = page.locator('[class*="switcher"]');
  const count = await switchers.count();
  for (let index = 0; index < count; index += 1) {
    const switcher = switchers.nth(index);
    const message = switcher.locator('xpath=ancestor::article[1]');
    if ((await message.count()) === 0) {
      continue;
    }
    const box = await switcher.boundingBox();
    const messageBox = await message.boundingBox();
    if (box === null || messageBox === null) {
      throw new Error('未取得翻页组件位置');
    }
    if (Math.abs(box.x + box.width - (messageBox.x + messageBox.width)) > 1) {
      throw new Error(`翻页组件未贴右: ${box.x + box.width} vs ${messageBox.x + messageBox.width}`);
    }
    if (box.y < messageBox.y + messageBox.height / 2) {
      throw new Error('翻页组件未在消息底部');
    }
  }
}

async function assertSessionMenuExpandsOnHover(page) {
  const button = page.locator('[class*="menuButton"]').first();
  if ((await button.count()) === 0) {
    return;
  }
  await button.hover();
  const panel = page.locator('[class*="menuPanel"]').first();
  await panel.waitFor({ state: 'visible' });
  const items = await panel.locator('button').count();
  if (items < 2) {
    throw new Error(`操作菜单项不足: ${items}`);
  }
  await page.mouse.move(0, 0);
}

async function assertWorkspaceRemoveRevealsOnHover(page) {
  const remove = page.getByLabel(/^移除 workspace：/).first();
  if ((await remove.count()) === 0) {
    return;
  }
  if (await remove.isVisible()) {
    throw new Error('移除 workspace 控件在悬停前就可见');
  }
  const add = page.getByLabel(/^在 .* 下新建 session$/).first();
  await add.hover();
  await remove.waitFor({ state: 'visible' });
  await page.mouse.move(0, 0);
}

async function assertNodeSwitcher(page, story) {
  const switcher = page.getByRole('navigation', { name: '节点切换器' });
  if ((await switcher.count()) === 0) {
    throw new Error('缺少节点切换器');
  }

  const pills = switcher.locator('button');
  const pillCount = await pills.count();
  if (emptyNodeStories.has(story.id) && pillCount !== 0) {
    throw new Error(`空节点表仍渲染了 ${pillCount} 个节点`);
  }
  if (!emptyNodeStories.has(story.id) && pillCount === 0) {
    throw new Error('节点切换器没有节点');
  }

  const switcherBox = await switcher.boundingBox();
  if (switcherBox === null) {
    throw new Error('未取得节点切换器尺寸');
  }
  for (let index = 0; index < pillCount; index += 1) {
    const box = await pills.nth(index).boundingBox();
    if (box === null) {
      throw new Error('未取得节点按钮尺寸');
    }
    if (box.x < switcherBox.x - 1 || box.x + box.width > switcherBox.x + switcherBox.width + 1) {
      throw new Error(`节点按钮溢出切换器: ${box.x}..${box.x + box.width}`);
    }
  }

  const selected = await switcher.locator('button[aria-pressed="true"]').count();
  const expected = noSelectionStories.has(story.id) ? 0 : 1;
  if (selected !== expected) {
    throw new Error(`选中节点数量不是 ${expected}: ${selected}`);
  }
}

const nodeStateHeadings = new Map([
  ['pages-console--unavailable-node', '节点不可用'],
  ['pages-console--unknown-node', '未知节点'],
  ['pages-console--no-nodes', '选择一个节点'],
]);

async function assertNodeStates(page, story) {
  const expected = nodeStateHeadings.get(story.id);
  if (expected === undefined) {
    return;
  }
  const heading = page.locator('h2').first();
  if ((await heading.count()) === 0) {
    throw new Error(`节点状态页缺少标题: ${story.id}`);
  }
  const text = await heading.textContent();
  if (text !== expected) {
    throw new Error(`节点状态页标题不是「${expected}」: ${text}`);
  }
  const add = page.getByRole('button', { name: '注册 workspace' });
  if ((await add.count()) === 0) {
    throw new Error('节点不可用的侧栏缺少注册 workspace 按钮');
  }
  if (!(await add.isDisabled())) {
    throw new Error('节点不可用时未禁用注册 workspace 命令');
  }
}

async function capture(page, target, story, theme, prefix = '') {
  const globals = theme === 'dark' ? '&globals=theme:dark' : '';
  try {
    await page.goto(
      `http://127.0.0.1:${address.port}/iframe.html?id=${story.id}&viewMode=story${globals}`,
    );
    await page.locator('#storybook-root > *').first().waitFor({ state: 'attached' });
    await page.evaluate(() => document.fonts.ready);
    await assertNoHorizontalOverflow(page);
    await assertBranchSwitcherPlacement(page);
    if (switcherStories(story.id)) {
      await assertNodeSwitcher(page, story);
    }
    await assertNodeStates(page, story);
    const hasComposer = (await page.locator('[class*="composer"]').count()) > 0;
    if (isConsolePage(story.id) && hasComposer) {
      await assertChromeInViewport(page);
      await assertColumnAlignment(page);
      await assertSessionMenuExpandsOnHover(page);
      await assertWorkspaceRemoveRevealsOnHover(page);
    }
    const file = join(target, `${prefix}${story.id.replaceAll('/', '-')}.png`);
    await page.screenshot({ path: file });
    const info = await stat(file);
    if (info.size === 0) {
      throw new Error('截图为空文件');
    }
  } catch (error) {
    throw new Error(`截图失败: ${theme} ${story.id}`, { cause: error });
  }
}

await rm(output, { recursive: true, force: true });
await mkdir(output, { recursive: true });
await mkdir(join(output, 'dark'), { recursive: true });
await mkdir(join(output, 'viewports'), { recursive: true });

const browser = await chromium.launch();

try {
  const page = await browser.newPage({
    viewport: { width: 1440, height: 960 },
    deviceScaleFactor: 2,
    reducedMotion: 'reduce',
  });

  for (const theme of ['light', 'dark']) {
    const target = theme === 'dark' ? join(output, 'dark') : output;
    for (const story of stories) {
      await capture(page, target, story, theme);
    }
  }

  for (const viewport of storyViewports) {
    await page.setViewportSize({ width: viewport.width, height: viewport.height });
    for (const theme of ['light', 'dark']) {
      for (const story of stories.filter((entry) => isConsolePage(entry.id))) {
        const target = join(output, 'viewports');
        await capture(page, target, story, theme, `${viewport.label}-${theme}-`);
      }
    }
  }
} finally {
  await browser.close();
  server.close();
}

console.log(`${stories.length * 2 + storyViewports.length * 2 * stories.filter((entry) => isConsolePage(entry.id)).length} 张截图写入 ${output}`);
