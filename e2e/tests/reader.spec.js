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

  test('search finds matches, jumps to them, and highlights all in section', async ({ page }) => {
    await page.goto(READER);
    await expect(frame(page).locator('h1')).toHaveText('Chapter 1');

    await page.fill('#search-input', 'xylophone');
    await page.press('#search-input', 'Enter');

    // "xylophone quartz xylophone" in chapter 2 → 2 results
    await expect(page.locator('.result-item')).toHaveCount(2);
    await expect(page.locator('.search-summary')).toHaveText('2 results');

    await page.locator('.result-item').first().click();
    await expect(page.locator('#section-num')).toHaveText('2');

    const marks = frame(page).locator('mark.epub-highlight');
    await expect(marks).toHaveCount(2);
    await expect(marks.first()).toHaveText('xylophone');
    await expect(marks.first()).toHaveCSS('background-color', 'rgb(255, 224, 138)');

    // Navigating away drops the highlights
    await page.locator('#next-btn').click();
    await page.locator('#prev-btn').click();
    await expect(frame(page).locator('mark.epub-highlight')).toHaveCount(0);

    // Escape clears results
    await page.focus('#search-input');
    await page.press('#search-input', 'Escape');
    await expect(page.locator('.result-item')).toHaveCount(0);
  });

  test('progress indicator appears and grows through the book', async ({ page }) => {
    await page.goto(READER);
    await expect(page.locator('#progress')).toHaveText(/^\d+%$/);
    await expect(page.locator('#progress')).toHaveText('0%');

    await page.locator('#next-btn').click();
    await page.locator('#next-btn').click();
    await expect(page.locator('#section-num')).toHaveText('3');

    const text = await page.locator('#progress').textContent();
    expect(parseInt(text, 10)).toBeGreaterThan(0);
  });

  test('reading position is saved in the URL and restored on load', async ({ page }) => {
    await page.goto(READER);
    await page.locator('#next-btn').click();
    await expect(page.locator('#section-num')).toHaveText('2');
    await expect(page).toHaveURL(/#loc=1:/);

    // Scroll deep into the long chapter; the fragment tracks it
    await frame(page).locator('#end').scrollIntoViewIfNeeded();
    await expect(page).toHaveURL(/#loc=1:0\.[5-9]|#loc=1:1\.0/);

    let pctDeep = 0;
    await expect
      .poll(async () => {
        pctDeep = parseInt(await page.locator('#progress').textContent(), 10);
        return pctDeep;
      })
      .toBeGreaterThan(20);

    // A fresh load of the saved URL restores section, scroll and progress
    const url = page.url();
    await page.goto('about:blank');
    await page.goto(url);
    await expect(page.locator('#section-num')).toHaveText('2');
    await expect
      .poll(() =>
        frame(page)
          .locator('body')
          .evaluate((el) => el.ownerDocument.scrollingElement.scrollTop),
      )
      .toBeGreaterThan(300);
    await expect
      .poll(async () => parseInt(await page.locator('#progress').textContent(), 10))
      .toBeGreaterThanOrEqual(pctDeep - 3);
  });

  test('mode switching keeps the reading position', async ({ page }) => {
    await page.goto(READER + '&section=1');
    await frame(page).locator('#end').scrollIntoViewIfNeeded();
    await expect(page).toHaveURL(/#loc=1:0\.[5-9]|#loc=1:1\.0/);

    await page.locator('#mode-single').click();
    await expect(frame(page).locator('.epub-section')).toHaveCount(3);
    // Still in section 2, deep into it
    await expect(page).toHaveURL(/#loc=1:0\.[3-9]|#loc=1:1\.0/);

    await page.locator('#mode-chapter').click();
    await expect(page.locator('#section-num')).toHaveText('2');
    await expect
      .poll(() =>
        frame(page)
          .locator('body')
          .evaluate((el) => el.ownerDocument.scrollingElement.scrollTop),
      )
      .toBeGreaterThan(100);
  });

  test('content column is horizontally centered', async ({ page }) => {
    await page.goto(READER);
    const gaps = await frame(page)
      .locator('body')
      .evaluate((el) => {
        const r = el.getBoundingClientRect();
        const vw = el.ownerDocument.documentElement.clientWidth;
        return { left: r.left, right: vw - r.right };
      });
    expect(Math.abs(gaps.left - gaps.right)).toBeLessThanOrEqual(2);
    expect(gaps.left).toBeGreaterThan(50);
  });

  test('open-another-book returns to the upload screen', async ({ page }) => {
    await page.goto(READER);
    await expect(page.locator('#reader-screen')).toBeVisible();

    await page.locator('#new-book-btn').click();
    await expect(page.locator('#upload-screen')).toBeVisible();
    await expect(page.locator('#reader-screen')).not.toBeVisible();
  });
});
