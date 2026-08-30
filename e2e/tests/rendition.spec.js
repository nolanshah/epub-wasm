// @ts-check
import { test, expect } from '@playwright/test';

const PAGE = '/rendition.html?load=fixture.epub';

const frame = (page) => page.frameLocator('#viewer iframe');

test.describe('Rendition (rendition.html)', () => {
  test('displays the book and reports location', async ({ page }) => {
    await page.goto(PAGE);

    await expect(page.locator('#status')).toContainText('Click Path Fixture — section 1/3');
    await expect(frame(page).locator('h1')).toHaveText('Chapter 1');

    // set_styles CSS injected
    await expect(frame(page).locator('body')).toHaveCSS('max-width', '640px');
  });

  test('next/prev via buttons and keyboard', async ({ page }) => {
    await page.goto(PAGE);
    await expect(page.locator('#status')).toContainText('section 1/3');

    await page.locator('#next').click();
    await expect(page.locator('#status')).toContainText('section 2/3');
    await expect(frame(page).locator('h1')).toHaveText('Chapter 2');

    await page.locator('#prev').click();
    await expect(page.locator('#status')).toContainText('section 1/3');

    await page.keyboard.press('ArrowRight');
    await expect(page.locator('#status')).toContainText('section 2/3');
    await page.keyboard.press('ArrowLeft');
    await expect(page.locator('#status')).toContainText('section 1/3');
  });

  test("Rendition's own click handler navigates internal links", async ({ page }) => {
    await page.goto(PAGE);
    await expect(frame(page).locator('h1')).toHaveText('Chapter 1');

    await frame(page).locator('#link-to-ch2-end').click();
    await expect(page.locator('#status')).toContainText('section 2/3');
    await expect(frame(page).locator('h1')).toHaveText('Chapter 2');
  });

  test('deep link &section=N uses display_section', async ({ page }) => {
    await page.goto(PAGE + '&section=2');
    await expect(page.locator('#status')).toContainText('section 3/3');
    await expect(frame(page).locator('h1')).toHaveText('Chapter 3');
  });
});
