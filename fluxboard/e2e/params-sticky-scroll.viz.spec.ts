import { test, expect } from '@playwright/test';

test.describe('Params sticky columns stay aligned while scrolling', () => {
  test('vertical scroll keeps pinned left cells aligned', async ({ page }) => {
    await page.setViewportSize({ width: 1440, height: 900 });
    await page.goto('http://localhost:5000/params');
    await page.waitForLoadState('networkidle');

    const header = page.locator('thead tr');
    const bodyRow = page.locator('tbody tr').first();
    await expect(header).toBeVisible();
    await expect(bodyRow).toBeVisible();

    // Identify the table scroll container (the div that directly wraps the table)
    const scroller = page
      .locator('div.flex-1.overflow-y-auto')
      .filter({ has: page.locator('table') })
      .first();

    // Ensure scroller exists; fall back to window scroll if not found
    const scrollerCount = await scroller.count();
    if (scrollerCount > 0) {
      await scroller.evaluate((el) => {
        el.scrollTop = 400;
      });
      await page.waitForTimeout(50);
    } else {
      await page.evaluate(() => window.scrollTo(0, 400));
      await page.waitForTimeout(50);
    }

    // Re-query the first visible body row after scroll
    const row = page.locator('tbody tr').first();
    await expect(row).toBeVisible();

    // Compare top positions between the pinned first cell and a far-right cell
    const pinnedCell = row.locator('td').first();
    const someCell = row.locator('td').nth(5); // one of the param columns

    const [pinnedBox, cellBox] = await Promise.all([
      pinnedCell.boundingBox(),
      someCell.boundingBox(),
    ]);

    expect(pinnedBox?.y).toBeDefined();
    expect(cellBox?.y).toBeDefined();

    const pinnedTop = Math.round(pinnedBox!.y!);
    const cellTop = Math.round(cellBox!.y!);
    // Allow 1px tolerance for subpixel rounding
    expect(Math.abs(pinnedTop - cellTop)).toBeLessThanOrEqual(1);

    // Ensure the pinned cell remains visible at the very left after horizontal scroll
    if (scrollerCount > 0) {
      await scroller.evaluate((el) => {
        el.scrollLeft = 600;
      });
      await page.waitForTimeout(50);
    } else {
      await page.evaluate(() => window.scrollTo(600, 400));
      await page.waitForTimeout(50);
    }

    const pinnedAfterScroll = await pinnedCell.boundingBox();
    expect(pinnedAfterScroll?.x).toBeLessThanOrEqual(8); // still stuck to left edge
  });

  test('header row remains visible after vertical scrolling', async ({ page }) => {
    await page.setViewportSize({ width: 1440, height: 900 });
    await page.goto('http://localhost:5000/params');
    await page.waitForLoadState('networkidle');

    const scroller = page
      .locator('div.flex-1.overflow-y-auto')
      .filter({ has: page.locator('table') })
      .first();

    const scrollerCount = await scroller.count();
    if (scrollerCount === 0) {
      test.fail(true, 'expected params table scroller to exist');
      return;
    }

    // Scroll far enough to push the first body rows out of the viewport.
    await scroller.evaluate((el) => {
      el.scrollTop = 600;
    });
    await page.waitForTimeout(50);

    const headerCell = page.locator('thead th').first();
    await expect(headerCell).toBeVisible();

    const [headerBox, scrollerBox, computedPosition, stickyTop] = await Promise.all([
      headerCell.boundingBox(),
      scroller.boundingBox(),
      headerCell.evaluate((el) => window.getComputedStyle(el).position),
      headerCell.evaluate((el) => window.getComputedStyle(el).top),
    ]);

    expect(headerBox).not.toBeNull();
    expect(scrollerBox).not.toBeNull();
    expect(computedPosition).toBe('sticky');
    expect(stickyTop).toBe('0px');

    // Header should hug the top edge of the scroll container (allowing 1px tolerance).
    const headerTop = Math.round(headerBox!.y!);
    const scrollerTop = Math.round(scrollerBox!.y!);
    expect(Math.abs(headerTop - scrollerTop)).toBeLessThanOrEqual(1);
  });
});
