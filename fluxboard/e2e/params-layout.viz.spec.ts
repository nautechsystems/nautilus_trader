import { test, expect } from '@playwright/test';

const viewports = [
  { width: 1280, height: 800 },
  { width: 1440, height: 900 },
  { width: 1680, height: 1000 },
];

test.describe('Params layout (pinned + widths)', () => {
  for (const vp of viewports) {
    test(`pinned alignment @${vp.width}`, async ({ page }) => {
      await page.setViewportSize(vp);
      await page.goto('http://localhost:5000/params');
      await page.waitForLoadState('networkidle');

      // Wait for header and first row to render
      const header = page.locator('thead tr');
      const firstRow = page.locator('tbody tr').first();
      await expect(header).toBeVisible();
      await expect(firstRow).toBeVisible();

      // Measure widths: Strategy, Run, Actions header cells
      const [w0, w1, w2] = await header.locator('th').all().then(async ths => {
        const els = await Promise.all(ths.slice(0, 3).map(t => t.elementHandle()));
        const boxes = await Promise.all(els.map(e => e!.boundingBox()));
        return boxes.map(b => Math.round((b?.width ?? 0)));
      });

      expect(w0).toBeGreaterThanOrEqual(275);
      expect(w0).toBeLessThanOrEqual(285);
      expect(w1).toBeGreaterThanOrEqual(78);
      expect(w1).toBeLessThanOrEqual(82);
      expect(w2).toBeGreaterThanOrEqual(90);
      expect(w2).toBeLessThanOrEqual(96);

      // Verify first param header and first row cell share the same width
      const paramHeader = header.locator('th').nth(3);
      const paramCell = firstRow.locator('td').nth(3);
      const [hw, cw] = await Promise.all([
        paramHeader.boundingBox().then(b => Math.round(b?.width ?? 0)),
        paramCell.boundingBox().then(b => Math.round(b?.width ?? 0)),
      ]);
      expect(Math.abs(hw - cw)).toBeLessThanOrEqual(1);

      // Screenshot pinned region for visual regression
      const clip = await header.boundingBox();
      if (clip) {
        await page.screenshot({
          path: `playwright-params-pinned-${vp.width}.png`,
          clip: { x: clip.x, y: clip.y, width: Math.min(clip.width, 380), height: clip.height + 60 },
        });
      }
    });
  }
});

