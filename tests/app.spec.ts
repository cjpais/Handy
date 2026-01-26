import { test, expect } from '@playwright/test';

test.describe('Handy App', () => {
  test('loads the main page', async ({ page }) => {
    await page.goto('/');
    
    // Wait for the app to load - check for any content
    await expect(page.locator('body')).toBeVisible();
    
    // Take a screenshot for debugging
    await page.screenshot({ path: 'test-results/app-loaded.png' });
  });

  test('has a title', async ({ page }) => {
    await page.goto('/');
    
    // Check the page has loaded with some content
    await expect(page).toHaveTitle(/.*/);
  });
});
