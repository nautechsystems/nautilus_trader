import { describe, it, expect, beforeEach } from 'vitest';
import {
  loadLayout,
  saveLayout,
  createLayoutsFromPreset,
  saveCollapsedPanels,
  loadCollapsedPanels,
} from './storage';

const LEGACY_KEY = 'fluxboard:dashboard:layout';
const namespacedKey = (preset: string, scope = 'default') => `${LEGACY_KEY}:${scope}:${preset}`;

describe('storage layout helpers', () => {
  beforeEach(() => {
    localStorage.clear();
  });

  it('returns preset layout when nothing stored', () => {
    const preset = 'default';
    const expected = createLayoutsFromPreset(preset);

    const result = loadLayout(preset);

    expect(result).toEqual(expected);
    expect(localStorage.getItem(namespacedKey(preset))).toBeNull();
  });

  it('persists and loads layout using namespaced key', () => {
    const preset = 'pilot';
    const layout = {
      lg: [
        { i: 'signal', x: 0, y: 0, w: 12, h: 3 },
        { i: 'trades', x: 0, y: 3, w: 12, h: 3 },
      ],
    };

    saveLayout(preset, layout);

    const stored = localStorage.getItem(namespacedKey(preset));
    expect(stored).toBeTruthy();
    const parsed = JSON.parse(stored as string);
    expect(parsed).toMatchObject({
      preset,
      version: expect.any(Number),
      layouts: layout,
    });

    const reloaded = loadLayout(preset);
    expect(reloaded.lg).toEqual(layout.lg);
    expect(reloaded.md).toEqual(layout.lg);
    expect(reloaded.sm).toEqual([
      { i: 'signal', x: 0, y: 0, w: 6, h: 3 },
      { i: 'trades', x: 0, y: 3, w: 6, h: 3 },
    ]);
    expect(reloaded.xs).toEqual([
      { i: 'signal', x: 0, y: 0, w: 4, h: 3 },
      { i: 'trades', x: 0, y: 3, w: 4, h: 3 },
    ]);
    expect(reloaded.xxs).toEqual([
      { i: 'signal', x: 0, y: 0, w: 1, h: 3 },
      { i: 'trades', x: 0, y: 3, w: 1, h: 3 },
    ]);
  });

  it('migrates legacy layout key to namespaced storage', () => {
    const legacyLayout = [
      { i: 'balances', x: 0, y: 0, w: 12, h: 3 },
    ];
    localStorage.setItem(LEGACY_KEY, JSON.stringify(legacyLayout));

    const result = loadLayout('custom');

    expect(result.lg).toEqual([
      { i: 'balances', x: 0, y: 0, w: 12, h: 3 },
    ]);
    expect(result.sm).toEqual([
      { i: 'balances', x: 0, y: 0, w: 6, h: 3 },
    ]);
    const migrated = localStorage.getItem(namespacedKey('custom'));
    expect(migrated).toBeTruthy();
    expect(JSON.parse(migrated as string)).toMatchObject({
      layouts: { lg: legacyLayout },
      preset: 'custom',
      version: expect.any(Number),
    });
  });

  it('falls back to preset when stored layout is malformed', () => {
    const preset = 'default';
    const expected = createLayoutsFromPreset(preset);
    localStorage.setItem(namespacedKey(preset), '{not-json');

    const result = loadLayout(preset);

    expect(result).toEqual(expected);
  });

  it('round-trips collapsed panel state', () => {
    const collapsed = new Set(['signal', 'balances']);

    saveCollapsedPanels(collapsed);

    const reloaded = loadCollapsedPanels();
    expect(reloaded.size).toBe(collapsed.size);
    expect([...reloaded]).toEqual(expect.arrayContaining([...collapsed]));
  });

  it('does not reuse unscoped layouts for non-default surfaces', () => {
    const legacyLayout = [
      { i: 'signal', x: 0, y: 0, w: 12, h: 3 },
      { i: 'trades', x: 0, y: 3, w: 12, h: 3 },
    ];
    localStorage.setItem(namespacedKey('default'), JSON.stringify(legacyLayout));

    const result = (loadLayout as any)('default', 'equities');

    expect(result).toEqual(createLayoutsFromPreset('default'));
  });

  it('round-trips collapsed panel state per surface scope', () => {
    const collapsed = new Set(['balances']);

    (saveCollapsedPanels as any)(collapsed, 'equities');

    const equitiesCollapsed = (loadCollapsedPanels as any)('equities');
    const tokenmmCollapsed = (loadCollapsedPanels as any)('tokenmm');

    expect([...equitiesCollapsed]).toEqual(['balances']);
    expect(tokenmmCollapsed.size).toBe(0);
  });
});

  it('loads layouts saved as raw Layouts object under namespaced key (preserves heights)', () => {
    const preset = 'default';
    const rawLayouts = {
      lg: [
        { i: 'signal', x: 0, y: 0, w: 12, h: 6 },
        { i: 'trades', x: 0, y: 6, w: 12, h: 4 },
      ],
    };

    // Simulate older code that saved raw Layouts directly under the namespaced key
    localStorage.setItem(namespacedKey(preset), JSON.stringify(rawLayouts));

    const result = loadLayout(preset);

    expect(result.lg).toEqual(rawLayouts.lg);
    expect(result.sm).toEqual([
      { i: 'signal', x: 0, y: 0, w: 6, h: 6 },
      { i: 'trades', x: 0, y: 6, w: 6, h: 4 },
    ]);
  });
