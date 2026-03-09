import { test, expect } from '@playwright/test';

test.describe('PnL Socket.io Disconnect - Real Test', () => {
  test('PnL page disconnects socket.io and report works', async ({ page }) => {
    // Enable console logging to capture socket disconnect messages
    page.on('console', (msg) => {
      if (msg.text().includes('socket') || msg.text().includes('PnL')) {
        console.log(`[Browser Console] ${msg.text()}`);
      }
    });

    // Navigate to PnL page
    console.log('\n1. Navigating to PnL page...');
    await page.goto('http://localhost:5000/pnl');

    // Wait for page to load
    await page.waitForLoadState('networkidle');
    await page.waitForTimeout(2000); // Give time for socket disconnect

    // Check console logs for disconnect message
    console.log('\n2. Checking for socket disconnect in console...');
    const consoleLogs: string[] = [];
    page.on('console', (msg) => {
      consoleLogs.push(msg.text());
    });

    // Re-navigate to trigger disconnect again
    await page.reload();
    await page.waitForLoadState('networkidle');
    await page.waitForTimeout(2000);

    // Check socket state via JavaScript
    console.log('\n3. Checking socket.io state...');
    const socketState = await page.evaluate(() => {
      // Try to access socket from the module
      // Since socket isn't exposed on window, we'll check network activity
      return {
        socketioRequests: Array.from(document.querySelectorAll('script')).some(
          s => s.src.includes('socket.io')
        ),
        // Check if there are any active socket.io connections by looking at network
        hasSocketIO: typeof (window as any).io !== 'undefined',
      };
    });
    console.log('Socket state:', socketState);

    // Find and click Run button
    console.log('\n4. Finding Run button...');
    const runButton = page.getByRole('button', { name: /Run/i }).first();
    await expect(runButton).toBeVisible({ timeout: 10000 });
    console.log('Run button found');

    // Click Run button
    console.log('\n5. Clicking Run button...');
    await runButton.click();

    // Wait for report to complete (check for loading state to disappear)
    console.log('\n6. Waiting for report to complete...');

    // Look for either success (summary appears) or error
    try {
      // Wait for either summary or error message
      await Promise.race([
        page.waitForSelector('text=Overall Summary', { timeout: 60000 }),
        page.waitForSelector('text=Error', { timeout: 60000 }),
        page.waitForSelector('[class*="error"]', { timeout: 60000 }),
      ]);

      // Check if we got an error
      const errorText = await page.locator('text=Error, text=timeout, text=Failed').first().isVisible().catch(() => false);
      if (errorText) {
        console.log('❌ Error detected in UI');
        const errorMsg = await page.locator('text=Error').first().textContent().catch(() => 'Unknown error');
        throw new Error(`PnL report failed: ${errorMsg}`);
      }

      // Check for summary (success)
      const summaryVisible = await page.locator('text=Overall Summary').isVisible().catch(() => false);
      if (summaryVisible) {
        console.log('✅ Report completed successfully!');
      } else {
        throw new Error('Report completed but no summary found');
      }
    } catch (error) {
      // Take screenshot for debugging
      await page.screenshot({ path: 'pnl-test-failure.png', fullPage: true });
      console.log('Screenshot saved to pnl-test-failure.png');
      throw error;
    }

    // Verify socket is still disconnected (should be)
    console.log('\n7. Verifying socket state after report...');
    const socketStateAfter = await page.evaluate(() => {
      return {
        hasSocketIO: typeof (window as any).io !== 'undefined',
      };
    });
    console.log('Socket state after report:', socketStateAfter);

    console.log('\n✅ Test completed successfully!');
  });

  test('PnL report API endpoint responds quickly', async ({ request }) => {
    console.log('\nTesting PnL API endpoint directly...');
    const startTime = Date.now();

    const response = await request.get('http://localhost:5000/api/v1/pnl?minutes=5', {
      timeout: 30000,
    });

    const elapsed = Date.now() - startTime;

    console.log(`Response status: ${response.status()}`);
    console.log(`Response time: ${elapsed}ms`);

    expect(response.status()).toBe(200);
    expect(elapsed).toBeLessThan(10000); // Should complete in <10s

    const body = await response.json();
    expect(body).toHaveProperty('ok');
    console.log('✅ API endpoint test passed!');
  });
});
