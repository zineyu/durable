import { expect, test } from './support/fixtures';
import { assistantMessages, composer, sendMessage, sidebar, workspaceGroup } from './support/ui';

test('注册 workspace 后创建 session，回答流式写入 transcript 并在刷新后保留', async ({
  page,
  app,
}) => {
  app.provider.push({ kind: 'hold', text: '已完成：workspace 检查通过。' });

  await page.goto(app.urlFor());
  await expect(page.getByRole('heading', { name: '选择一个 session' })).toBeVisible();
  await expect(page.getByText(app.workspace)).toBeVisible();

  await sidebar(page).getByRole('button', { name: '注册 workspace' }).click();
  const dialog = page.getByRole('dialog', { name: '注册 workspace' });
  await dialog.getByLabel('workspace 路径').fill(app.extraWorkspace);
  await dialog.getByRole('button', { name: '注册' }).click();
  await expect(dialog).toHaveCount(0);

  const group = workspaceGroup(page, app.extraWorkspace);
  await expect(group).toBeVisible();
  await group.getByLabel(`在 ${app.extraWorkspace} 下新建 session`).click();

  await expect(composer(page)).toBeVisible();
  await expect(page.getByRole('heading', { level: 1 })).toHaveText('新 session');

  await sendMessage(page, '检查 workspace 根目录');

  await expect(assistantMessages(page)).toHaveCount(1);
  await expect(assistantMessages(page)).toHaveAttribute('data-state', 'streaming');
  await expect(page.getByText('生成中')).toBeVisible();
  await expect(page.getByRole('button', { name: '取消' })).toBeVisible();
  await expect(page.getByText('已完成：workspace 检查通过。')).toBeVisible();

  app.provider.release();

  await expect(assistantMessages(page)).toHaveAttribute('data-state', 'complete');
  await expect(page.getByRole('button', { name: '取消' })).toHaveCount(0);
  await expect(page.getByRole('heading', { level: 1 })).toHaveText('检查 workspace 根目录');
  await expect(page.locator('button[aria-current="true"]')).toContainText('检查 workspace 根目录');

  await page.reload();

  await expect(assistantMessages(page)).toHaveAttribute('data-state', 'complete');
  await expect(page.getByText('已完成：workspace 检查通过。')).toBeVisible();
  await expect(page.getByRole('heading', { level: 1 })).toHaveText('检查 workspace 根目录');
});

test('工具调用渲染为可折叠卡片并展示结果', async ({ page, app }) => {
  app.provider.push({
    kind: 'tool-calls',
    calls: [{ id: 'call-1', name: 'read_file', arguments: '{"path":"README.md"}' }],
  });
  app.provider.push({ kind: 'text', text: 'README 说明这是端到端测试的 workspace。' });

  const sessionId = await app.createSession(app.workspace);
  await page.goto(app.urlFor(sessionId));
  await sendMessage(page, '读 README.md');

  const card = page.locator('details').filter({ hasText: 'read_file' });
  await expect(card).toBeVisible();
  await expect(card.getByText('成功')).toBeVisible();
  await expect(card.locator('summary').getByText('README.md', { exact: true })).toBeVisible();

  await card.locator('summary').click();

  await expect(card.getByText('入参')).toBeVisible();
  await expect(card.getByText('结果')).toBeVisible();
  await expect(card.locator('pre').last()).toContainText('hello e2e');

  await expect(page.getByText('README 说明这是端到端测试的 workspace。')).toBeVisible();
  await expect(assistantMessages(page)).toHaveAttribute('data-state', 'complete');
});
