import { defineConfig } from '@playwright/test';

const baseURL = process.env.E2E_BASE_URL || 'http://localhost:5000';

export default defineConfig({
  testDir: './e2e',
  testMatch: '*.spec.ts',
  timeout: 60_000,
  reporter: 'list',
  use: {
    baseURL,
    headless: true,
    ignoreHTTPSErrors: true,
  },
});

