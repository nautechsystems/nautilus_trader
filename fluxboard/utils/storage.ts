// LocalStorage helpers for dashboard layout persistence

import type { Layouts, Layout } from 'react-grid-layout';
import { PRESETS, type LayoutConfig } from '../components/layout/presets';
import { SOUND } from '../constants';

const LAYOUT_KEY_PREFIX = 'fluxboard:dashboard:layout';
const COLLAPSE_KEY = 'fluxboard:dashboard:collapsed';
const CURRENT_LAYOUT_VERSION = 2;
const DEFAULT_STORAGE_SCOPE = 'default';

type StoredLayouts = {
  version: number;
  preset: string;
  layouts: Layouts;
};

const BREAKPOINT_KEYS: Array<keyof Layouts> = ['lg', 'md', 'sm', 'xs', 'xxs'];
const BREAKPOINT_COLS: Record<(typeof BREAKPOINT_KEYS)[number], number> = {
  lg: 12,
  md: 12,
  sm: 6,
  xs: 4,
  xxs: 1,
};

function clampLayout(layout: Array<LayoutConfig | Layout> = [], cols: number): Layout[] {
  if (!layout.length) {
    return [];
  }

  return layout.map(item => {
    const maxWidth = Math.max(1, Math.min(item.w ?? cols, cols));
    const maxX = Math.max(cols - maxWidth, 0);
    return {
      ...item,
      w: maxWidth,
      h: Math.max(1, item.h ?? 1),
      x: Math.min(Math.max(item.x ?? 0, 0), maxX),
      y: Math.max(item.y ?? 0, 0),
    };
  });
}

function presetLayoutsForAllBreakpoints(preset: string): Layouts {
  const presetLayout = PRESETS[preset] || PRESETS.default;
  const layouts: Layouts = {} as Layouts;
  BREAKPOINT_KEYS.forEach(bp => {
    const cols = BREAKPOINT_COLS[bp] ?? 12;
    layouts[bp] = clampLayout(presetLayout, cols);
  });
  return layouts;
}

function ensureLayouts(value: unknown, preset: string): Layouts {
  const fallbackLayouts = presetLayoutsForAllBreakpoints(preset);

  if (!value) {
    return fallbackLayouts;
  }

  if (Array.isArray(value)) {
    return presetLayoutsForAllBreakpointsFromArray(value);
  }

  if (typeof value === 'object' && value !== null) {
    const asAny = value as any;

    // Support both wrapped StoredLayouts ({ layouts: { lg, ... } })
    // and raw Layouts objects saved directly under the key ({ lg, md, ... }).
    const candidateLayouts: Layouts | null =
      (asAny.layouts && typeof asAny.layouts === 'object'
        ? (asAny.layouts as Layouts)
        : (asAny as Layouts));

    if (candidateLayouts && typeof candidateLayouts === 'object') {
      const hasAnyBreakpoint = BREAKPOINT_KEYS.some(
        bp => Array.isArray((candidateLayouts as any)[bp])
      );

      if (hasAnyBreakpoint) {
        const normalized: Layouts = {} as Layouts;
        BREAKPOINT_KEYS.forEach(bp => {
          const cols = BREAKPOINT_COLS[bp] ?? 12;
          const source =
            (candidateLayouts[bp] as Array<LayoutConfig | Layout> | undefined)
            ?? (candidateLayouts.lg as Array<LayoutConfig | Layout> | undefined)
            ?? fallbackLayouts[bp];
          normalized[bp] = clampLayout(source, cols);
        });
        return normalized;
      }
    }
  }

  return fallbackLayouts;
}

function presetLayoutsForAllBreakpointsFromArray(layout: Array<LayoutConfig | Layout>): Layouts {
  const layouts: Layouts = {} as Layouts;
  BREAKPOINT_KEYS.forEach(bp => {
    const cols = BREAKPOINT_COLS[bp] ?? 12;
    layouts[bp] = clampLayout(layout, cols);
  });
  return layouts;
}

function normalizeStorageScope(scope?: string): string {
  return String(scope || DEFAULT_STORAGE_SCOPE);
}

function usesLegacyScopeFallback(scope?: string): boolean {
  return normalizeStorageScope(scope) === DEFAULT_STORAGE_SCOPE;
}

function layoutStorageKey(preset: string, scope?: string): string {
  return `${LAYOUT_KEY_PREFIX}:${normalizeStorageScope(scope)}:${preset}`;
}

function legacyLayoutStorageKey(preset: string): string {
  return `${LAYOUT_KEY_PREFIX}:${preset}`;
}

function collapsedStorageKey(scope?: string): string {
  return `${COLLAPSE_KEY}:${normalizeStorageScope(scope)}`;
}

export function createLayoutsFromPreset(preset: string): Layouts {
  return presetLayoutsForAllBreakpoints(preset);
}

export function saveLayout(preset: string, layouts: Layouts, scope?: string) {
  try {
    const payload: StoredLayouts = {
      version: CURRENT_LAYOUT_VERSION,
      preset,
      layouts,
    };
    localStorage.setItem(layoutStorageKey(preset, scope), JSON.stringify(payload));
  } catch (e) {
    if (import.meta.env?.DEV) {
      console.warn('[storage] Failed to save layout:', e);
    }
  }
}

export function loadLayout(preset: string, scope?: string): Layouts {
  try {
    const namespaced = localStorage.getItem(layoutStorageKey(preset, scope));
    if (namespaced) {
      return ensureLayouts(JSON.parse(namespaced), preset);
    }

    if (usesLegacyScopeFallback(scope)) {
      const legacyNamespaced = localStorage.getItem(legacyLayoutStorageKey(preset));
      if (legacyNamespaced) {
        const layouts = ensureLayouts(JSON.parse(legacyNamespaced), preset);
        saveLayout(preset, layouts, scope);
        return layouts;
      }

      // Legacy fallback (pre-versioned key)
      const legacy = localStorage.getItem(LAYOUT_KEY_PREFIX);
      if (legacy) {
        const layouts = ensureLayouts(JSON.parse(legacy), preset);
        saveLayout(preset, layouts, scope);
        return layouts;
      }
    }
  } catch (e) {
    if (import.meta.env?.DEV) {
      console.warn('[storage] Failed to load layout:', e);
    }
  }
  return createLayoutsFromPreset(preset);
}

export function saveCollapsedPanels(panels: Set<string>, scope?: string) {
  try {
    localStorage.setItem(collapsedStorageKey(scope), JSON.stringify(Array.from(panels)));
  } catch (e) {
    if (import.meta.env?.DEV) {
      console.warn('[storage] Failed to save collapsed panels:', e);
    }
  }
}

export function loadCollapsedPanels(scope?: string): Set<string> {
  try {
    const stored = localStorage.getItem(collapsedStorageKey(scope));
    if (stored) {
      const parsed = JSON.parse(stored);
      if (Array.isArray(parsed)) {
        return new Set(parsed);
      }
    }

    if (usesLegacyScopeFallback(scope)) {
      const legacy = localStorage.getItem(COLLAPSE_KEY);
      if (legacy) {
        const parsed = JSON.parse(legacy);
        if (Array.isArray(parsed)) {
          const panels = new Set(parsed);
          saveCollapsedPanels(panels, scope);
          return panels;
        }
      }
    }
  } catch (e) {
    if (import.meta.env?.DEV) {
      console.warn('[storage] Failed to load collapsed panels:', e);
    }
  }
  return new Set();
}

/**
 * Get sound muted preference from localStorage
 * @returns true if muted, false if enabled (default: false)
 */
export function getSoundMuted(): boolean {
  try {
    const stored = localStorage.getItem(SOUND.STORAGE_KEY);
    return stored === 'true';
  } catch (e) {
    if (import.meta.env?.DEV) {
      console.debug('[storage] Failed to read sound muted:', e);
    }
    return false;  // Default: sound enabled
  }
}

/**
 * Set sound muted preference in localStorage
 * @param muted - true to mute, false to enable
 */
export function setSoundMuted(muted: boolean): void {
  try {
    localStorage.setItem(SOUND.STORAGE_KEY, String(muted));
  } catch (e) {
    if (import.meta.env?.DEV) {
      console.warn('[storage] Failed to save sound muted:', e);
    }
  }
}
