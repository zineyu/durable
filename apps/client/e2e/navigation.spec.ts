import { expect, test } from './support/fixtures';
import { composer, hostSwitcher } from './support/ui';

test('没有选中 session 时提示选择，未知 session 显示独立状态', async ({ page, app }) => {
  await page.goto(app.urlFor());
  await expect(page.getByRole('heading', { name: '选择一个 session' })).toBeVisible();
  await expect(composer(page)).toHaveCount(0);

  await page.goto(`${app.agentUrl}/#/s/missing-session`);

  await expect(page.getByRole('heading', { name: '未知 session' })).toBeVisible();
  await expect(page.getByText('session missing-session 不在当前状态里。')).toBeVisible();
  await expect(composer(page)).toHaveCount(0);

  const sessionId = await app.createSession(app.workspace);
  await page.goto(app.urlFor(sessionId));

  await expect(composer(page)).toBeVisible();
  await expect(page.getByRole('heading', { level: 1 })).toHaveText('新 session');
  await expect(hostSwitcher(page).getByRole('button', { name: 'local' })).toBeVisible();
  await expect(page).toHaveURL(new RegExp(`#/s/${sessionId}$`));
});
