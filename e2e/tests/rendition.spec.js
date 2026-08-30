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

test.describe('paginated flow', () => {
  /** Extract {page, count} from the status line, e.g. "page 2/9". */
  const pageInfo = async (page) => {
    const text = await page.locator('#status').textContent();
    const m = text?.match(/page (\d+)\/(\d+)/);
    return m ? { page: Number(m[1]), count: Number(m[2]) } : null;
  };

  test('long chapter splits into pages; next/prev move by page', async ({ page }) => {
    await page.goto(PAGE + '&section=2'); // chapter 3 is long
    await expect(page.locator('#status')).toContainText('section 3/3');

    await page.locator('#flow').click();
    await expect(page.locator('#status')).toContainText('page 1/');

    const info = await pageInfo(page);
    expect(info.count).toBeGreaterThan(1);

    // Content is shifted by the page stride, not scrolled
    await page.locator('#next').click();
    await expect(page.locator('#status')).toContainText('page 2/');
    const transform = await frame(page).locator('body').evaluate(
      (el) => getComputedStyle(el).transform,
    );
    expect(transform).not.toBe('none');
    expect(transform).toMatch(/matrix\(1, 0, 0, 1, -\d/);

    await page.locator('#prev').click();
    await expect(page.locator('#status')).toContainText('page 1/');
  });

  test('page boundaries chain into section boundaries', async ({ page }) => {
    await page.goto(PAGE + '&section=2');
    await page.locator('#flow').click();
    await expect(page.locator('#status')).toContainText('page 1/');

    // prev at page 1 → previous section, on its LAST page
    await page.locator('#prev').click();
    await expect(page.locator('#status')).toContainText('section 2/3');
    const info = await pageInfo(page);
    expect(info.count).toBeGreaterThan(1);
    expect(info.page).toBe(info.count);

    // next from the last page → back to section 3, page 1
    await page.locator('#next').click();
    await expect(page.locator('#status')).toContainText('section 3/3');
    await expect(page.locator('#status')).toContainText('page 1/');
  });

  test('display_href with a fragment lands on the right page', async ({ page }) => {
    await page.goto(PAGE);
    await page.locator('#flow').click();
    await expect(page.locator('#status')).toContainText('section 1/3');

    // #end sits at the bottom of the long chapter 2
    await page.evaluate(() => window.rendition.display_href('OEBPS/Text/ch 2.xhtml#end'));

    await expect(page.locator('#status')).toContainText('section 2/3');
    const info = await pageInfo(page);
    expect(info.count).toBeGreaterThan(1);
    expect(info.page).toBeGreaterThan(1);
  });

  test('internal links still navigate in paginated flow', async ({ page }) => {
    await page.goto(PAGE);
    await page.locator('#flow').click();
    await expect(frame(page).locator('h1')).toHaveText('Chapter 1');

    await frame(page).locator('#link-to-ch2-end').click();
    await expect(page.locator('#status')).toContainText('section 2/3');

    // switching back to scrolled flow re-renders without pagination
    await page.locator('#flow').click();
    await expect(page.locator('#status')).not.toContainText('page');
  });
});
