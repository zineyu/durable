import { expect, test } from './support/fixtures';
import type { Page } from '@playwright/test';
import type { Harness } from './support/harness';
import { composer, hostSwitcher, sidebar } from './support/ui';

const modifier = process.platform === 'darwin' ? 'Meta' : 'Control';

/// 经 host 面板注册一个本机 host，并等它连上。
async function registerHost(page: Page, app: Harness, name: string): Promise<void> {
  await hostSwitcher(page).getByRole('button', { name: '注册 host' }).click();
  const dialog = page.getByRole('dialog', { name: '注册 host' });
  await expect(dialog).toBeVisible();
  await dialog.getByLabel('host 名').fill(name);
  await dialog.getByRole('button', { name: '注册' }).click();
  await expect(dialog).toHaveCount(0);
  await expect
    .poll(async () => (await app.bootstrap()).hosts.find((host) => host.name === name)?.status)
    .toBe('ok');
}

test('命令面板按 host 分组搜索 session 并跳转', async ({ page, app }) => {
  const localSession = await app.createSession(app.workspace);
  await page.goto(app.urlFor(localSession));

  await registerHost(page, app, 'e2e-host-b');
  await app.registerWorkspace('e2e-host-b', app.workspace);
  const remoteSession = await app.createSession(app.workspace, 'e2e-host-b');

  await page.keyboard.press(`${modifier}+k`);
  const palette = page.getByRole('dialog', { name: '命令面板' });
  await expect(palette).toBeVisible();

  await palette.getByRole('combobox', { name: '搜索命令' }).fill('e2e-host-b');
  const option = palette.getByRole('option').filter({ hasText: 'e2e-host-b ·' }).first();
  await expect(option).toBeVisible();
  await option.click();

  await expect(page).toHaveURL(new RegExp(`#/s/${remoteSession}`));
  await expect(composer(page)).toBeVisible();
});

test('Alt+数字跳到第 n 个 host 最近的 session，问号显示快捷键清单', async ({ page, app }) => {
  await page.goto(app.urlFor());
  await registerHost(page, app, 'e2e-host-b');
  await app.registerWorkspace('e2e-host-b', app.workspace);
  const remoteSession = await app.createSession(app.workspace, 'e2e-host-b');

  await page.goto(app.urlFor());
  await expect(page.getByRole('heading', { name: '选择一个 session' })).toBeVisible();

  await page.keyboard.press('Alt+2');
  await expect(page).toHaveURL(new RegExp(`#/s/${remoteSession}`));
  await expect(composer(page)).toBeVisible();

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
