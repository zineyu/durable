import { expect, test } from './support/fixtures';
import { composer, hostSwitcher, sidebar, workspaceGroup } from './support/ui';

/// 规格 user story R-19、R-47：不可用 host 的会话列表为空并给出说明，
/// 写操作禁用，注册 workspace 的 host 默认为当前选中的 host。
test('不可达 host 的说明与禁用，注册 workspace 默认选中 host', async ({ page, app }) => {
  await app.registerHost('lap2', 'local', null);
  await expect
    .poll(async () => (await app.bootstrap()).hosts.find((host) => host.name === 'lap2')?.status)
    .toBe('ok');
  await app.registerWorkspace('lap2', app.extraWorkspace);
  await app.createSession(app.workspace, 'local', '本机会话');

  const sessionId = await app.createSession(app.extraWorkspace, 'lap2', 'lap2 会话');
  await page.goto(app.urlFor(sessionId));
  await expect(composer(page)).toBeVisible();
  await expect(sidebar(page).getByRole('button', { name: /^lap2 会话/ })).toBeVisible();

  await sidebar(page).getByRole('button', { name: '注册 workspace' }).click();
  const dialog = page.getByRole('dialog', { name: '注册 workspace' });
  await expect(dialog.getByLabel('host 名')).toHaveValue('lap2');
  await dialog.getByRole('button', { name: '取消' }).click();
  await expect(dialog).toHaveCount(0);

  await page.evaluate(() => {
    window.localStorage.clear();
  });
  await page.goto(app.urlFor());
  await expect(composer(page)).toHaveCount(0);

  await app.setHostEnabled('local', false);

  const group = workspaceGroup(page, app.workspace);
  await expect(group.getByText('host 已停用')).toBeVisible();
  await expect(group.getByLabel(`在 ${app.workspace} 下新建 session`)).toBeDisabled();
  await expect(group.getByLabel(`移除 workspace：${app.workspace}`)).toBeDisabled();
  await expect(sidebar(page).getByRole('button', { name: /^本机会话/ })).toHaveCount(0);

  await hostSwitcher(page).getByRole('button', { name: 'host', exact: true }).click();
  const panel = page.getByRole('region', { name: 'host local' });
  await expect(panel.getByText('host 已停用')).toBeVisible();
  await expect(panel.getByRole('button', { name: '重命名 host：local' })).toBeDisabled();
  await expect(panel.getByRole('button', { name: '删除 host：local' })).toBeDisabled();
  await expect(panel.getByRole('button', { name: '启用' })).toBeEnabled();
});
