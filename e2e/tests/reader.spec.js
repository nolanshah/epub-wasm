// @ts-check
import { test, expect } from '@playwright/test';

const READER = '/index.html?load=fixture.epub';

/** The reader's content iframe. */
const frame = (page) => page.frameLocator('#epub-container iframe');

test.describe('client reader (index.html)', () => {
  test('loads a book: metadata, TOC, first section, resources', async ({ page }) => {
    await page.goto(READER);

    await expect(page.locator('#book-title')).toHaveText('Click Path Fixture');
    await expect(page.locator('#book-author')).toHaveText('epub-wasm tests');
    await expect(page.locator('#section-total')).toHaveText('3');

    // TOC: 5 entries incl. the non-linking <span> part heading
    await expect(page.locator('.toc-item')).toHaveCount(5);
    await expect(page.locator('.toc-item').nth(1)).toHaveText('Part Two');

    // Section 1 rendered in the iframe
    await expect(frame(page).locator('h1')).toHaveText('Chapter 1');

    // Image arrives via blob URL created in Rust
    const img = frame(page).locator('#dot');
    await expect(img).toBeVisible();
    await expect(img).toHaveJSProperty('naturalWidth', 1);
    const src = await img.getAttribute('src');
    expect(src).toMatch(/^blob:/);

    // Stylesheet (also a blob URL) actually applied
    await expect(frame(page).locator('.opener')).toHaveCSS('color', 'rgb(0, 90, 200)');
  });

  test('TOC click navigates, incl. fragment target in a percent-encoded file', async ({ page }) => {
    await page.goto(READER);
    await expect(frame(page).locator('h1')).toHaveText('Chapter 1');

    await page.locator('.toc-item', { hasText: 'Chapter Two, ending' }).click();

    await expect(page.locator('#section-num')).toHaveText('2');
    await expect(frame(page).locator('h1')).toHaveText('Chapter 2');
    // Active TOC entry updates (both entries point at ch 2)
    await expect(page.locator('.toc-item.active').first()).toHaveText('Chapter Two');
  });

  test('internal link inside the content navigates sections', async ({ page }) => {
    await page.goto(READER);

    await frame(page).locator('#link-to-ch2-end').click();
    await expect(page.locator('#section-num')).toHaveText('2');
    await expect(frame(page).locator('h1')).toHaveText('Chapter 2');

    await frame(page).locator('#link-to-ch1').click();
    await expect(page.locator('#section-num')).toHaveText('1');
    await expect(frame(page).locator('h1')).toHaveText('Chapter 1');
  });

  test('external links open in a new tab and scripts are stripped', async ({ page }) => {
    await page.goto(READER);

    const ext = frame(page).locator('#external-link');
    await expect(ext).toHaveAttribute('target', '_blank');
    await expect(ext).toHaveAttribute('href', 'https://example.com');

    const scripts = await frame(page).locator('script').count();
    expect(scripts).toBe(0);
  });

  test('next/prev buttons and arrow keys', async ({ page }) => {
    await page.goto(READER);
    await expect(frame(page).locator('h1')).toHaveText('Chapter 1');

    await page.locator('#next-btn').click();
    await expect(page.locator('#section-num')).toHaveText('2');

    await page.locator('#next-btn').click();
    await expect(page.locator('#section-num')).toHaveText('3');
    await expect(frame(page).locator('h1')).toHaveText('Chapter 3');

    // At the end, next is a no-op
    await page.locator('#next-btn').click();
    await expect(page.locator('#section-num')).toHaveText('3');

    await page.locator('#prev-btn').click();
    await expect(page.locator('#section-num')).toHaveText('2');

    await page.keyboard.press('ArrowLeft');
    await expect(page.locator('#section-num')).toHaveText('1');
    await page.keyboard.press('ArrowRight');
    await expect(page.locator('#section-num')).toHaveText('2');
  });

  test('single-page mode shows all sections; chapter mode restores', async ({ page }) => {
    await page.goto(READER);
    await expect(frame(page).locator('h1')).toHaveText('Chapter 1');

    await page.locator('#mode-single').click();
    await expect(frame(page).locator('.epub-section')).toHaveCount(3);
    await expect(frame(page).locator('h1').nth(2)).toHaveText('Chapter 3');
    await expect(page.locator('#next-btn')).toBeDisabled();

    // Internal link in single mode scrolls instead of reloading
    await frame(page).locator('#link-to-ch2-end').click();
    await expect(frame(page).locator('.epub-section')).toHaveCount(3);

    await page.locator('#mode-chapter').click();
    await expect(page.locator('#next-btn')).toBeEnabled();
    await expect(frame(page).locator('h1')).toHaveCount(1);
  });

  test('deep links: &section=N and &mode=single', async ({ page }) => {
    await page.goto(READER + '&section=1');
    await expect(page.locator('#section-num')).toHaveText('2');
    await expect(frame(page).locator('h1')).toHaveText('Chapter 2');

    await page.goto(READER + '&mode=single');
    await expect(frame(page).locator('.epub-section')).toHaveCount(3);
  });

  test('open-another-book returns to the upload screen', async ({ page }) => {
    await page.goto(READER);
    await expect(page.locator('#reader-screen')).toBeVisible();

    await page.locator('#new-book-btn').click();
    await expect(page.locator('#upload-screen')).toBeVisible();
    await expect(page.locator('#reader-screen')).not.toBeVisible();
  });
});
