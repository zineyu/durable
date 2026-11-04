import { expect, test } from './support/fixtures';
import { assistantMessages, sendMessage } from './support/ui';

test('agent 重启后客户端显示重连中并自动回到已连接，事件流继续可用', async ({ page, app }) => {
  app.provider.push({ kind: 'text', text: '重连前的回答。' });
  app.provider.push({ kind: 'text', text: '重连后的回答。' });

  const sessionId = await app.createSession(app.workspace);
  await page.goto(app.urlFor(sessionId));
  await expect(page.getByText('已连接')).toBeVisible();

  await sendMessage(page, '重启前的消息');
  await expect(page.getByText('重连前的回答。')).toBeVisible();
  await expect(assistantMessages(page)).toHaveAttribute('data-state', 'complete');

  await app.restartAgent();

  await expect(page.getByText('重连中')).toBeVisible();
  await expect(page.getByText('已连接')).toBeVisible();
  await expect(page.getByText('重连前的回答。')).toBeVisible();

  await sendMessage(page, '重启后的消息');
  await expect(page.getByText('重连后的回答。')).toBeVisible();
  await expect(assistantMessages(page)).toHaveCount(2);
});

test('agent 长时间离线后连接状态升级为已断开，恢复后回到已连接', async ({ page, app }) => {
  const sessionId = await app.createSession(app.workspace);
  await page.goto(app.urlFor(sessionId));
  await expect(page.getByText('已连接')).toBeVisible();

  await app.stopAgent();

  await expect(page.getByText('已断开')).toBeVisible();
  await expect(page.getByText('重连中')).toHaveCount(0);

  await app.startAgent();

  await expect(page.getByText('已连接')).toBeVisible();
  await expect(page.getByText('已断开')).toHaveCount(0);
});
