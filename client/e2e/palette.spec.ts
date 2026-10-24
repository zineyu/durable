import { expect, test } from './support/fixtures';
import { composer, nodeSwitcher, sidebar } from './support/ui';

const modifier = process.platform === 'darwin' ? 'Meta' : 'Control';

test('命令面板跨节点搜索 session 并跳转', async ({ page, app }) => {
  await app.addNode('e2e-node-b');
  const sessionId = await app.createSession(app.workspace);

  await page.goto(app.urlFor());
  await expect(composer(page)).toHaveCount(0);

  await page.keyboard.press(`${modifier}+k`);
  const palette = page.getByRole('dialog', { name: '命令面板' });
  await expect(palette).toBeVisible();

  await palette.getByRole('combobox', { name: '搜索命令' }).fill('e2e-node-b');
  const option = palette.getByRole('option').filter({ hasText: 'e2e-node-b ·' }).first();
  await expect(option).toBeVisible();
  await option.click();

  await expect(page).toHaveURL(new RegExp(`#/n/e2e-node-b/s/${sessionId}`));
  await expect(
    nodeSwitcher(page).getByRole('button', { name: 'e2e-node-b', exact: true }),
  ).toHaveAttribute('aria-pressed', 'true');
  await expect(composer(page)).toBeVisible();
});

test('Alt+数字切换节点，问号显示快捷键清单', async ({ page, app }) => {
  await app.addNode('e2e-node-b');
  await page.goto(app.urlFor());
  await expect(
    nodeSwitcher(page).getByRole('button', { name: app.nodeName, exact: true }),
  ).toHaveAttribute('aria-pressed', 'true');

  await page.keyboard.press('Alt+2');
  await expect(page).toHaveURL(/#\/n\/e2e-node-b$/);
  await expect(
    nodeSwitcher(page).getByRole('button', { name: 'e2e-node-b', exact: true }),
  ).toHaveAttribute('aria-pressed', 'true');
  await expect(page.getByRole('heading', { name: '选择一个 session' })).toBeVisible();

  await page.keyboard.type('?');
  const help = page.getByRole('dialog', { name: '快捷键' });
  await expect(help).toBeVisible();
  await expect(help.getByText('Ctrl/⌘ + K')).toBeVisible();
  await expect(help.getByText('Alt + 1…9')).toBeVisible();

  await page.keyboard.press('Escape');
  await expect(help).toHaveCount(0);
});

test('Esc 关闭对话框而不取消运行', async ({ page, app }) => {
  await page.goto(app.urlFor());
  await sidebar(page).getByRole('button', { name: '注册 workspace' }).click();
  const dialog = page.getByRole('dialog', { name: '注册 workspace' });
  await expect(dialog).toBeVisible();

  await page.keyboard.press('Escape');
  await expect(dialog).toHaveCount(0);
});
