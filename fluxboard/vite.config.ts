import { defineConfig } from 'vite';
import { configDefaults } from 'vitest/config';
import react from '@vitejs/plugin-react';
import path from 'path';

const DEFAULT_DEV_HOST = '127.0.0.1';
const DEFAULT_DEV_PORT = 5173;
const DEFAULT_PREVIEW_PORT = 4173;
const DEFAULT_FLUXAPI_PORT = '5022';
const DEFAULT_FLUXBOARD_BASE_PATH = '/tokenmm/';

function toPort(rawValue: string | undefined, fallback: number): number {
  const parsed = Number.parseInt(String(rawValue ?? ''), 10);
  return Number.isFinite(parsed) && parsed > 0 ? parsed : fallback;
}

function normalizeBasePath(rawValue: string | undefined, fallback: string): string {
  const trimmed = (rawValue || '').trim();
  if (!trimmed) {
    return fallback;
  }
  const prefixed = trimmed.startsWith('/') ? trimmed : `/${trimmed}`;
  return prefixed.endsWith('/') ? prefixed : `${prefixed}/`;
}

function parseAllowedHosts(rawValue: string | undefined): string[] {
  const baseHosts = ['localhost', '127.0.0.1', '::1'];
  const extraHosts = (rawValue || '')
    .split(',')
    .map((value) => value.trim())
    .filter(Boolean);
  return Array.from(new Set([...baseHosts, ...extraHosts]));
}

const fullVitestSuite = process.env.VITEST_FULL === '1';

const quarantinedGlobs = [
  'e2e/**',                      // Playwright suites (run via pnpm test:e2e)
  'Alerts.test.tsx',             // Requires realtime backend + timers
  'Trades.test.tsx',             // Relies on socket + audio APIs
  'components/SignalTable.test.tsx',
  'components/domain/**/*.test.ts?(x)',
  '__tests__/trades*.test.tsx',
  '__tests__/pnl*.test.tsx',
  '__tests__/PnL.test.tsx',
  '__tests__/pnl-*.test.tsx',
  '__tests__/components/**',
  '__tests__/panels/**',
  '__tests__/stores/**',
  '__tests__/components/domain/**',
  '__tests__/ui/Dialog.test.tsx',
  '__tests__/ui/Popover.test.tsx',
];

const testExclude = fullVitestSuite
  ? configDefaults.exclude
  : [...configDefaults.exclude, ...quarantinedGlobs];

export default defineConfig(({ command }) => {
  const isDevServer = command === 'serve';
  const fluxApiScheme = (process.env.FLUXAPI_SCHEME || 'http').trim();
  const fluxApiHost = (process.env.FLUXAPI_HOST || DEFAULT_DEV_HOST).trim();
  const fluxApiPort = (process.env.FLUXAPI_PORT || DEFAULT_FLUXAPI_PORT).trim();
  const fluxapiUrl =
    (process.env.FLUXAPI_URL || '').trim()
    || `${fluxApiScheme}://${fluxApiHost}:${fluxApiPort}`;
  const allowedHosts = parseAllowedHosts(process.env.VITE_ALLOWED_HOSTS);

  return {
    base: isDevServer
      ? '/'
      : normalizeBasePath(process.env.FLUXBOARD_BASE_PATH, DEFAULT_FLUXBOARD_BASE_PATH),
    plugins: [react()],
    resolve: {
      alias: {
        '@': path.resolve(__dirname, '.'),
      },
    },
    test: {
      environment: 'jsdom',
      globals: true,
      setupFiles: ['./vitest.setup.ts'],
      exclude: testExclude,
      // Limit workers to prevent excessive memory usage on multi-core systems
      // jsdom is memory-intensive, so we cap at 4 workers even on 32-core systems
      maxWorkers: process.env.CI ? 2 : 4,
      minWorkers: 1,
      // Use threads pool (default) but with limited workers
      pool: 'threads',
      poolOptions: {
        threads: {
          singleThread: false,
          // Limit threads to prevent memory exhaustion
          maxThreads: process.env.CI ? 2 : 4,
          minThreads: 1,
        },
      },
    },
    server: {
      host: (process.env.VITE_DEV_HOST || DEFAULT_DEV_HOST).trim(),
      port: toPort(process.env.VITE_DEV_PORT, DEFAULT_DEV_PORT),
      strictPort: true,
      allowedHosts,
      proxy: {
        '/api': {
          target: fluxapiUrl,
          changeOrigin: true,
        },
        '/socket.io': {
          target: fluxapiUrl,
          ws: true,
          changeOrigin: true,
        },
      },
    },
    preview: {
      host: (process.env.VITE_PREVIEW_HOST || DEFAULT_DEV_HOST).trim(),
      port: toPort(process.env.VITE_PREVIEW_PORT, DEFAULT_PREVIEW_PORT),
      strictPort: true,
      allowedHosts,
    },
  };
});
