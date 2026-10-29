import { expect, test } from './support/fixtures';
import { composer } from './support/ui';

test('草稿按 session 保留，回到入口时恢复上次打开的 session', async ({ page, app }) => {
  const first = await app.createSession(app.workspace);
  const second = await app.createSession(app.workspace);

  await page.goto(app.urlFor(first));
  await composer(page).fill('半截草稿');

  await page.goto(app.urlFor(second));
  await expect(composer(page)).toHaveValue('');

  await page.goto(app.urlFor(first));
  await expect(composer(page)).toHaveValue('半截草稿');

  await page.goto(app.urlFor());
  await expect(page).toHaveURL(new RegExp(`#/s/${first}$`));
});
