import { describe, expect, it } from 'vitest';

import {
  buildProfilePath,
  getProfileDefinition,
  getUiSurface,
  resolvePathProfile,
  resolvePathnameProfile,
} from './uiProfiles';

describe('uiProfiles', () => {
  it('resolves default path profile for empty/unknown segments', () => {
    expect(resolvePathProfile(undefined)).toBe('default');
    expect(resolvePathProfile('')).toBe('default');
    expect(resolvePathProfile('unknown')).toBe('default');
  });

  it('maps token aliases to canonical tokenmm profile', () => {
    expect(resolvePathProfile('tokenmm')).toBe('tokenmm');
    expect(resolvePathProfile('tokenm')).toBe('tokenmm');
  });

  it('maps equities segment to equities profile', () => {
    expect(resolvePathProfile('equities')).toBe('equities');
  });

  it('maps lp segment to lp profile', () => {
    expect(resolvePathProfile('lp')).toBe('lp');
  });

  it('exposes stable maker profile definitions', () => {
    expect(getProfileDefinition('tokenmm')).toMatchObject({
      profile: 'tokenmm',
      aliases: ['tokenmm', 'tokenm'],
      basePath: '/tokenmm',
    });
    expect(getProfileDefinition('equities')).toMatchObject({
      profile: 'equities',
      aliases: ['equities'],
      basePath: '/equities',
    });
    expect(getProfileDefinition('lp')).toMatchObject({
      profile: 'lp',
      aliases: ['lp'],
      basePath: '/lp',
    });
  });

  it('resolves profile consistently from pathname', () => {
    expect(resolvePathnameProfile('/tokenmm/trades')).toBe('tokenmm');
    expect(resolvePathnameProfile('/tokenm/signal')).toBe('tokenmm');
    expect(resolvePathnameProfile('/equities/alerts')).toBe('equities');
    expect(resolvePathnameProfile('/lp/hedger')).toBe('lp');
    expect(resolvePathnameProfile('/trades')).toBe('default');
    expect(resolvePathnameProfile(undefined)).toBe('default');
  });

  it('exposes tokenmm nav/routes with alerts and without order-view', () => {
    const surface = getUiSurface('tokenmm');
    expect(surface.routePaths).toEqual([
      '/',
      '/dashboard',
      '/params',
      '/signal',
      '/trades',
      '/balances',
      '/alerts',
    ]);
    expect(surface.navLinks).toEqual([
      { path: '/', label: 'Dashboard' },
      { path: '/signal', label: 'Signal' },
      { path: '/params', label: 'Params' },
      { path: '/balances', label: 'Balances' },
      { path: '/trades', label: 'Trades' },
      { path: '/alerts', label: 'Alerts' },
    ]);
    expect(surface.routePaths).not.toContain('/order-view');
    expect(surface.navLinks).not.toContainEqual({ path: '/order-view', label: 'Orders' });
    expect(surface.allowedPanels).toEqual(['signal', 'params', 'balances', 'trades', 'alerts']);
    expect(surface.externalLinks).toEqual([]);
  });

  it('exposes maker-core nav/routes for equities', () => {
    const surface = getUiSurface('equities');
    expect(surface.routePaths).toEqual([
      '/',
      '/dashboard',
      '/params',
      '/signal',
      '/trades',
      '/balances',
      '/alerts',
    ]);
    expect(surface.allowedPanels).toEqual(['signal', 'params', 'balances', 'trades', 'alerts']);
    expect(surface.externalLinks).toEqual([]);
  });

  it('retains broader default surface without standalone equities route', () => {
    const surface = getUiSurface('default');
    expect(surface.routePaths).not.toContain('/equities');
    expect(surface.routePaths).toContain('/market-data');
    expect(surface.routePaths).not.toContain('/hedger');
    expect(surface.routePaths).not.toContain('/scanners');
    expect(surface.routePaths).not.toContain('/scanners-harness');
    expect(surface.navLinks).not.toContainEqual({ path: '/scanners', label: 'Scanners' });
    expect(surface.externalLinks.length).toBeGreaterThan(0);
  });

  it('exposes dedicated lp hedger surface', () => {
    const surface = getUiSurface('lp');
    expect(surface.homeRoutePath).toBe('/hedger');
    expect(surface.routePaths).toEqual(['/', '/hedger']);
    expect(surface.navLinks).toEqual([{ path: '/', label: 'Hedger' }]);
    expect(surface.externalLinks).toEqual([]);
  });

  it('builds profile-scoped paths', () => {
    expect(buildProfilePath('default', '/')).toBe('/');
    expect(buildProfilePath('default', '/signal')).toBe('/signal');
    expect(buildProfilePath('tokenmm', '/')).toBe('/tokenmm');
    expect(buildProfilePath('tokenmm', '/signal')).toBe('/tokenmm/signal');
    expect(buildProfilePath('equities', '/alerts')).toBe('/equities/alerts');
    expect(buildProfilePath('lp', '/')).toBe('/lp');
    expect(buildProfilePath('lp', '/hedger')).toBe('/lp/hedger');
  });
});
