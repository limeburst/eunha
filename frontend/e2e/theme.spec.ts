import { expect, test } from '@playwright/test'

test('theme switcher opens and toggles dark mode', async ({ page }) => {
  await page.goto('/')
  const html = page.locator('html')

  // Open the dropdown (regression guard: this stayed hidden on React 18).
  await page.getByRole('button', { name: 'Toggle theme' }).click()
  await expect(page.getByRole('menuitem', { name: 'Dark' })).toBeVisible()

  // Switch to Dark.
  await page.getByRole('menuitem', { name: 'Dark' }).click()
  await expect(html).toHaveClass(/dark/)

  // The class carries the page's own colours; `color-scheme` is what tells the
  // browser to draw *its* parts dark too. Without it Windows and Linux leave
  // every scrollbar — the window's, the advanced layout's panes, a scrolling
  // menu — light over a dark page. macOS's overlay scrollbars hide that, so
  // only an assertion catches it.
  await expect(html).toHaveCSS('color-scheme', 'dark')

  // Switch back to Light.
  await page.getByRole('button', { name: 'Toggle theme' }).click()
  await page.getByRole('menuitem', { name: 'Light' }).click()
  await expect(html).not.toHaveClass(/dark/)
  await expect(html).toHaveCSS('color-scheme', 'light')
})

test('renders the header with a sign-in action when logged out', async ({
  page,
}) => {
  await page.goto('/')
  await expect(page.getByRole('button', { name: 'Sign in' })).toBeVisible()
})
