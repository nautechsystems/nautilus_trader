import { execFileSync } from 'node:child_process';
import { mkdtempSync, readFileSync, rmSync } from 'node:fs';
import { createRequire } from 'node:module';
import { tmpdir } from 'node:os';
import path from 'node:path';
import React from 'react';
import { describe, expect, it, vi } from 'vitest';

vi.mock('react-dom/client', () => ({
  createRoot: () => ({ render: vi.fn() }),
}));

import { buildFluxboardChildRoutes, buildFluxboardTopLevelRoutes, buildTokenmmAliasTarget } from './main';
import { getUiSurface } from './config/uiProfiles';

const require = createRequire(import.meta.url);
const viteBinPath = path.join(path.dirname(require.resolve('vite/package.json')), 'bin', 'vite.js');

describe('main route builder', () => {
  it('registers top-level profile, alias, and catch-all routes', () => {
    const routes = buildFluxboardTopLevelRoutes();
    expect(routes.map((route) => route.path)).toEqual([
      '/',
      '/tokenm',
      '/tokenm/*',
      '/tokenmm',
      '/equities',
      '/lp',
      '*',
    ]);
  });

  it('uses redirect handlers for tokenm aliases and catch-all', () => {
    const routes = buildFluxboardTopLevelRoutes();

    const tokenmRoute = routes.find((route) => route.path === '/tokenm');
    const tokenmSplatRoute = routes.find((route) => route.path === '/tokenm/*');
    const tokenmmRoute = routes.find((route) => route.path === '/tokenmm');
    const equitiesRoute = routes.find((route) => route.path === '/equities');
    const lpRoute = routes.find((route) => route.path === '/lp');
    const catchAllRoute = routes.find((route) => route.path === '*');

    expect(tokenmRoute).toBeDefined();
    expect(tokenmSplatRoute).toBeDefined();
    expect(tokenmmRoute).toBeDefined();
    expect(equitiesRoute).toBeDefined();
    expect(lpRoute).toBeDefined();
    expect(catchAllRoute).toBeDefined();

    const tokenmElement = tokenmRoute?.element as React.ReactElement;
    const tokenmSplatElement = tokenmSplatRoute?.element as React.ReactElement;
    const tokenmmElement = tokenmmRoute?.element as React.ReactElement;
    const equitiesElement = equitiesRoute?.element as React.ReactElement;
    const lpElement = lpRoute?.element as React.ReactElement;
    const catchAllElement = catchAllRoute?.element as React.ReactElement<{ to: string; replace?: boolean }>;

    expect(tokenmElement.type).toBe(tokenmSplatElement.type);
    expect(tokenmElement.type).not.toBe(tokenmmElement.type);
    expect(tokenmElement.type).not.toBe(equitiesElement.type);
    expect(tokenmElement.type).not.toBe(lpElement.type);
    expect(catchAllElement.props.to).toBe('/');
    expect(catchAllElement.props.replace).toBe(true);
  });

  it('redirects unknown default child routes back to root', () => {
    const routes = buildFluxboardChildRoutes(getUiSurface('default'), {
      includeScannersHarness: false,
      fallbackPath: '/',
    });
    const wildcard = routes.find((route) => route.path === '*');
    const element = wildcard?.element as React.ReactElement<{ to: string }>;
    expect(element.props.to).toBe('/');
  });

  it('redirects unknown tokenmm child routes to /tokenmm', () => {
    const routes = buildFluxboardChildRoutes(getUiSurface('tokenmm'), {
      includeScannersHarness: false,
      fallbackPath: '/tokenmm',
    });
    const wildcard = routes.find((route) => route.path === '*');
    const element = wildcard?.element as React.ReactElement<{ to: string }>;
    expect(element.props.to).toBe('/tokenmm');
  });

  it('does not expose legacy standalone equities route as a child route', () => {
    const routes = buildFluxboardChildRoutes(getUiSurface('default'), {
      includeScannersHarness: false,
      fallbackPath: '/',
    });
    expect(routes.find((route) => route.path === 'equities')).toBeUndefined();
  });

  it('does not expose parked scanners routes on the default surface', () => {
    const routes = buildFluxboardChildRoutes(getUiSurface('default'), {
      includeScannersHarness: true,
      fallbackPath: '/',
    });

    expect(routes.find((route) => route.path === 'scanners')).toBeUndefined();
    expect(routes.find((route) => route.path === 'scanners-harness')).toBeUndefined();
  });

  it('exposes alerts but not order-view route on tokenmm surface', () => {
    const defaultRoutes = buildFluxboardChildRoutes(getUiSurface('default'), {
      includeScannersHarness: false,
      fallbackPath: '/',
    });
    const tokenmmRoutes = buildFluxboardChildRoutes(getUiSurface('tokenmm'), {
      includeScannersHarness: false,
      fallbackPath: '/tokenmm',
    });
    const equitiesRoutes = buildFluxboardChildRoutes(getUiSurface('equities'), {
      includeScannersHarness: false,
      fallbackPath: '/equities',
    });
    const lpRoutes = buildFluxboardChildRoutes(getUiSurface('lp'), {
      includeScannersHarness: false,
      fallbackPath: '/lp',
    });

    expect(defaultRoutes.find((route) => route.path === 'alerts')).toBeDefined();
    expect(tokenmmRoutes.find((route) => route.path === 'alerts')).toBeDefined();
    expect(equitiesRoutes.find((route) => route.path === 'alerts')).toBeDefined();
    expect(defaultRoutes.find((route) => route.path === 'dashboard')).toBeDefined();
    expect(tokenmmRoutes.find((route) => route.path === 'dashboard')).toBeDefined();
    expect(equitiesRoutes.find((route) => route.path === 'dashboard')).toBeDefined();
    expect(lpRoutes.find((route) => route.path === 'dashboard')).toBeUndefined();
    expect(lpRoutes.find((route) => route.path === 'hedger')).toBeDefined();

    expect(defaultRoutes.find((route) => route.path === 'order-view')).toBeUndefined();
    expect(tokenmmRoutes.find((route) => route.path === 'order-view')).toBeUndefined();
    expect(equitiesRoutes.find((route) => route.path === 'order-view')).toBeUndefined();
    expect(lpRoutes.find((route) => route.path === 'order-view')).toBeUndefined();
  });

  it('preserves tokenm splat path when redirecting to tokenmm', () => {
    expect(buildTokenmmAliasTarget('signal')).toBe('/tokenmm/signal');
    expect(buildTokenmmAliasTarget('alerts/deep/path')).toBe('/tokenmm/alerts/deep/path');
  });

  it('treats /lp as the lp hedger home surface', () => {
    const routes = buildFluxboardChildRoutes(getUiSurface('lp'), {
      includeScannersHarness: false,
      fallbackPath: '/lp',
    });

    expect(routes[0]?.element).toBeTruthy();
    expect(getUiSurface('lp').homeRoutePath).toBe('/hedger');
  });

  it('preserves query and hash in tokenm alias redirect', () => {
    expect(buildTokenmmAliasTarget('signal', '?foo=1', '#section')).toBe('/tokenmm/signal?foo=1#section');
    expect(buildTokenmmAliasTarget(undefined, '?foo=1', '#section')).toBe('/tokenmm?foo=1#section');
  });

  it('pins production builds to the shared static fluxboard base path', () => {
    const viteConfigSource = readFileSync(`${process.cwd()}/vite.config.ts`, 'utf-8');

    expect(viteConfigSource).toContain("const DEFAULT_FLUXBOARD_BASE_PATH = '/static/fluxboard/';");
    expect(viteConfigSource).toMatch(/base:\s*isDevServer\s*\?\s*'\/'\s*:\s*DEFAULT_FLUXBOARD_BASE_PATH/);
    expect(viteConfigSource).not.toContain('normalizeBasePath(process.env.FLUXBOARD_BASE_PATH');
  });

  it('emits shared static asset paths in production build output', async () => {
    const outDir = mkdtempSync(path.join(tmpdir(), 'fluxboard-build-'));

    try {
      execFileSync(
        process.execPath,
        [viteBinPath, 'build', '--outDir', outDir, '--emptyOutDir'],
        {
          cwd: process.cwd(),
          env: process.env,
          stdio: 'pipe',
        },
      );

      const html = readFileSync(path.join(outDir, 'index.html'), 'utf-8');
      expect(html).toContain('/static/fluxboard/assets/');
      expect(html).not.toContain('/tokenmm/assets/');
      expect(html).not.toContain('/equities/assets/');
    } finally {
      rmSync(outDir, { recursive: true, force: true });
    }
  }, 15000);
});
