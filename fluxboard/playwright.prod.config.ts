import { defineConfig } from '@playwright/test';

export default defineConfig({
  testDir: './e2e',
  testMatch: '*.spec.ts',
  timeout: 60_000,
  reporter: 'html',
  use: {
    baseURL: 'http://localhost:5000',
    headless: true,
    ignoreHTTPSErrors: true,
  },
  webServer: {
    command: 'pnpm preview -- --strictPort',
    port: 5000,
    reuseExistingServer: !process.env.CI,
    timeout: 120_000,
  },
});
