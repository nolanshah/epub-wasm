// @ts-check
import { test, expect } from '@playwright/test';

const frame = (page) => page.frameLocator('#viewer iframe');

const pageInfo = async (page) => {
  const text = await page.locator('#status').textContent();
  const m = text?.match(/page (\d+)\/(\d+)/);
  return m ? { page: Number(m[1]), count: Number(m[2]) } : null;
};

test.describe('RTL books', () => {
  const PAGE = '/rendition.html?load=fixture-rtl.epub';

  test('paginated flow flips the paging transform for dir=rtl content', async ({ page }) => {
    await page.goto(PAGE + '&section=1'); // the long RTL section
    await expect(page.locator('#status')).toContainText('section 2/2');

    await page.locator('#flow').click();
    await expect(page.locator('#status')).toContainText('page 1/');
    const info = await pageInfo(page);
    expect(info.count).toBeGreaterThan(1);

    await page.locator('#next').click();
    await expect(page.locator('#status')).toContainText('page 2/');

    // RTL columns extend leftward: advancing translates by a POSITIVE x
    const transform = await frame(page)
      .locator('body')
      .evaluate((el) => getComputedStyle(el).transform);
    expect(transform).toMatch(/matrix\(1, 0, 0, 1, \d/);
  });

  test('fragment jump lands on a late page, not silently page 1', async ({ page }) => {
    await page.goto(PAGE);
    await page.locator('#flow').click();
    await expect(page.locator('#status')).toContainText('section 1/2');

    await page.evaluate(() => window.rendition.display_href('OEBPS/r2.xhtml#end'));

    await expect(page.locator('#status')).toContainText('section 2/2');
    const info = await pageInfo(page);
    expect(info.count).toBeGreaterThan(1);
    expect(info.page).toBeGreaterThan(1);
  });

  test('arrow keys flip to match the reading direction', async ({ page }) => {
    await page.goto(PAGE);
    await expect(page.locator('#status')).toContainText('section 1/2');

    await page.keyboard.press('ArrowLeft'); // forward in RTL
    await expect(page.locator('#status')).toContainText('section 2/2');
    await page.keyboard.press('ArrowRight'); // back
    await expect(page.locator('#status')).toContainText('section 1/2');
  });

  test('client reader flips arrows too', async ({ page }) => {
    await page.goto('/index.html?load=fixture-rtl.epub');
    await expect(page.locator('#section-num')).toHaveText('1');

    await page.keyboard.press('ArrowLeft');
    await expect(page.locator('#section-num')).toHaveText('2');
    await page.keyboard.press('ArrowRight');
    await expect(page.locator('#section-num')).toHaveText('1');
  });
});

test.describe('fixed layout', () => {
  const PAGE = '/rendition.html?load=fixture-fxl.epub';

  test('pages scale to fit the viewport and navigate as sections', async ({ page }) => {
    await page.goto(PAGE);
    await expect(page.locator('#status')).toContainText('section 1/2');
    await expect(frame(page).locator('#label')).toHaveText('PAGE 1');

    const transform = await frame(page)
      .locator('body')
      .evaluate((el) => getComputedStyle(el).transform);
    expect(transform).toMatch(/^matrix\(/);
    const scale = parseFloat(transform.match(/matrix\((-?[\d.]+)/)[1]);
    expect(scale).toBeGreaterThan(0);
    expect(scale).not.toBe(1);

    await page.locator('#next').click();
    await expect(frame(page).locator('#label')).toHaveText('PAGE 2');
    await expect(page.locator('#status')).toContainText('section 2/2');
  });

  test('flow toggle never injects columns into FXL content', async ({ page }) => {
    await page.goto(PAGE);
    await expect(frame(page).locator('#label')).toHaveText('PAGE 1');

    await page.locator('#flow').click(); // request paginated
    await expect(frame(page).locator('#label')).toHaveText('PAGE 1');

    const style = await frame(page).locator('body').evaluate((el) => {
      const cs = getComputedStyle(el);
      return { columnWidth: cs.columnWidth, transform: cs.transform };
    });
    expect(style.columnWidth).toBe('auto');
    expect(style.transform).toMatch(/^matrix\(/); // still scaled

    // layout is reported
    const layout = await page.evaluate(() => window.rendition.layout);
    expect(layout).toBe('pre-paginated');
  });
});
