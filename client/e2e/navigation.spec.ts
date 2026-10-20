import { expect, test } from './support/fixtures';
import { nodeSwitcher, sidebar } from './support/ui';

test('未选择节点时提示选择节点，节点未知时提示未知节点', async ({ page, app }) => {
  await page.goto(app.hubUrl);
  await expect(page.getByRole('heading', { name: '选择一个节点' })).toBeVisible();

  await page.goto(`${app.hubUrl}/#/n/missing-node`);

  await expect(page.getByRole('heading', { name: '未知节点' })).toBeVisible();
  await expect(page.getByText('节点 missing-node 不在节点表里。它可能已被删除或改名。')).toBeVisible();
  await expect(sidebar(page).getByRole('button', { name: '注册 workspace' })).toBeDisabled();

  await nodeSwitcher(page).getByRole('button', { name: app.nodeName }).click();
  await expect(page.getByRole('heading', { name: '选择一个 session' })).toBeVisible();
  await expect(sidebar(page).getByRole('button', { name: '注册 workspace' })).toBeEnabled();
});
