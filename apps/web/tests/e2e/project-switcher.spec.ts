import { expect, test } from '@playwright/test';

// "Project: <name>" at the far left of the navbar switches the whole
// dashboard between configured projects and reaches the import and
// create forms.

// Routes that fetch from the fixture server must not outlive the test.
test.afterEach(async ({ page }) => { await page.unrouteAll({ behavior: 'ignoreErrors' }); });

test.beforeEach(async ({ page }) => {
  await page.route('**/api/profiles', async (route) => {
    const profiles = await (await route.fetch()).json();
    await route.fulfill({ json: [...profiles, { ...profiles[0], name: 'second', display_name: 'Second', repo: 'owner/second' }] });
  });
});

test('the switcher changes the project for every page and keeps it in the URL', async ({ page }) => {
  await page.goto('/?page=overview');
  const switcher = page.getByRole('button', { name: /^Project:/ });
  await expect(switcher).toHaveText(/Project: Fixture/);
  await switcher.click();
  const menu = page.getByRole('menu', { name: 'Projects' });
  await expect(menu.getByRole('menuitemradio', { name: /^Fixture/ })).toHaveAttribute('aria-checked', 'true');
  await menu.getByRole('menuitemradio', { name: /^Second/ }).click();
  await expect(switcher).toHaveText(/Project: Second/);
  await expect.poll(() => new URL(page.url()).searchParams.get('profile')).toBe('second');
  await expect(menu).toHaveCount(0);
  // The Profile sidebar follows the switcher rather than carrying its own selector.
  await page.getByRole('button', { name: 'Profile', exact: true }).click();
  const current = page.getByRole('region', { name: 'Current project' });
  await expect(current).toContainText('Second');
  await expect(current.getByRole('combobox')).toHaveCount(0);
});

test('a fresh dashboard opens the first checkout and remembers the new project', async ({ page }) => {
  const configured: object[] = [];
  await page.route('**/api/profiles', async route => {
    if (route.request().method() === 'POST') {
      configured.push(route.request().postDataJSON());
      return route.fulfill({ status: 201, json: { success: true } });
    }
    return route.fulfill({ json: configured });
  });
  await page.goto('/');
  await expect(page.getByRole('heading', { name: 'Add your first project' })).toBeVisible();
  await expect(page.getByRole('button', { name: /^Project:/ })).toHaveText(/Project: none/);
  await page.getByRole('button', { name: 'Use a local checkout' }).click();
  const form = page.getByRole('complementary', { name: 'Profile' });
  for (const [label, value] of [['Display Name *', 'First project'], ['Profile Name (ID) *', 'first'],
    ['Repository *', 'owner/first'], ['Local Path *', '/srv/first'], ['Artifact Root *', '/srv/artifacts']]) {
    await form.getByLabel(label, { exact: true }).fill(value);
  }
  await form.getByRole('button', { name: 'Add Profile', exact: true }).last().click();
  await expect(page.getByRole('button', { name: /^Project:/ })).toHaveText(/Project: First project/);
  await expect.poll(() => new URL(page.url()).searchParams.get('profile')).toBe('first');
  await page.reload();
  await expect(page.getByRole('button', { name: /^Project:/ })).toHaveText(/Project: First project/);
  await expect(page.getByRole('heading', { name: 'Add your first project' })).toHaveCount(0);
});

test('a failed project listing can be retried into first-project setup', async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  let healthy = false;
  await page.route('**/api/profiles', route => route.fulfill(healthy
    ? { json: [] } : { status: 503, json: { message: 'Project listing unavailable' } }));
  await page.goto('/');
  await expect(page.getByRole('alert').filter({ hasText: 'Project listing unavailable' })).toBeVisible();
  await expect(page.getByRole('heading', { name: 'Add your first project' })).toHaveCount(0);
  healthy = true;
  await page.getByRole('alert').getByRole('button', { name: 'Retry', exact: true }).click();
  await page.getByRole('button', { name: 'Import from Git', exact: true }).click();
  await expect(page.getByRole('button', { name: 'Import repository' })).toBeVisible();
});

test('the GitHub dropdown opens the repository pages', async ({ page }) => {
  await page.goto('/?page=overview');
  // Navbar order: Project, then Overview … Fleet, then GitHub.
  const left = async (name: string | RegExp) => (await page.getByRole('banner').getByRole('button', { name, exact: typeof name === 'string' }).first().boundingBox())!.x;
  expect(await left(/^Project:/)).toBeLessThan(await left('Overview'));
  // Primary navigation scrolls on compact desktops. Bring the last item
  // into view before checking its position against the fixed provider menu.
  const fleet = page.getByRole('banner').getByRole('button', { name: 'Fleet', exact: true });
  await expect.poll(async () => {
    await fleet.scrollIntoViewIfNeeded();
    const bounds = (await fleet.boundingBox())!;
    return bounds.x + bounds.width <= await left('GitHub');
  }).toBe(true);
  await page.getByRole('button', { name: 'GitHub', exact: true }).click();
  const menu = page.getByRole('menu', { name: 'GitHub pages' });
  await expect(menu.getByRole('menuitem', { name: 'Issues' })).toHaveAttribute('href', /\/issues$/);
  await expect(menu.getByRole('menuitem', { name: 'Pull requests' })).toHaveAttribute('href', /\/pulls$/);
  // Old Projects links land on Git now.
  await page.goto('/?page=projects&profile=fixture');
  await expect(page.getByRole('heading', { name: 'Git', exact: true })).toBeVisible();
});

test('Import from Git opens the Projects import form and Create new opens the add profile form', async ({ page }) => {
  await page.goto('/?page=overview');
  await page.getByRole('button', { name: /^Project:/ }).click();
  await page.getByRole('menuitem', { name: 'Import from Git' }).click();
  // The import form is the chat rail's; the chat opens expanded to show it.
  await expect(page).toHaveURL(/[?&]page=chat/);
  const rail = page.getByRole('complementary', { name: 'Chat navigation' });
  await expect(rail.locator('details').filter({ hasText: 'Import from Git' })).toHaveAttribute('open', '');
  await expect(rail.getByRole('button', { name: 'Import repository' })).toBeVisible();

  await page.getByRole('button', { name: /^Project:/ }).click();
  await page.getByRole('menuitem', { name: 'Create new' }).click();
  const profilePanel = page.getByRole('complementary', { name: 'Profile' });
  await expect(profilePanel.getByRole('heading', { name: 'Profile Management' })).toBeVisible();
  await expect(profilePanel.getByRole('heading', { name: 'Add New Profile' })).toBeVisible();
  await expect(profilePanel.getByPlaceholder('/path/to/local/repo')).toBeVisible();
});
