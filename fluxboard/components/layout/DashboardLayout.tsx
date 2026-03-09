// Dashboard layout with responsive react-grid-layout

import { useMemo, useRef, useState, useEffect, type CSSProperties } from 'react';
import { Responsive, type Layout, type Layouts } from 'react-grid-layout';
import 'react-grid-layout/css/styles.css';
import 'react-resizable/css/styles.css';
import { PRESETS } from './presets';
import { PANEL_REGISTRY, type PanelId } from './PanelRegistry';
import { useIsMobile } from '@/hooks/useIsMobile';
import { MobileDashboard } from './MobileDashboard';
import {
  createLayoutsFromPreset,
  loadCollapsedPanels,
  loadLayout,
  saveCollapsedPanels,
  saveLayout,
} from '../../utils/storage';
import { colors, spacing, borderRadius, typography } from '../../lib/tokens';

const ALL_PANELS: PanelId[] = Object.keys(PANEL_REGISTRY) as PanelId[];
const ResponsiveGridLayout = Responsive;

const BREAKPOINTS = {
  lg: 1200,
  md: 996,
  sm: 768,
  xs: 480,
  xxs: 0,
} as const;

const COLS = {
  lg: 12,
  md: 12,
  sm: 6,
  xs: 4,
  xxs: 1,
} as const;

type BreakpointKey = keyof typeof BREAKPOINTS;
const BREAKPOINT_ORDER: BreakpointKey[] = ['lg', 'md', 'sm', 'xs', 'xxs'];
const PREVENT_COLLISION = false;

type StyleWithVars = CSSProperties & Record<string, string | number>;

const toolbarStyle: CSSProperties = {
  gap: spacing.gap.sm,
  padding: spacing.gap.xs,
  backgroundColor: colors.bg.surface,
  borderBottom: `1px solid ${colors.border.DEFAULT}`,
};

const toolbarLabelStyle: CSSProperties = {
  fontSize: typography.fontSize['2xs'],
  color: colors.text.muted,
  textTransform: 'uppercase',
  letterSpacing: '0.08em',
};

const toolbarActionsStyle: CSSProperties = {
  marginLeft: 'auto',
  display: 'flex',
  flexWrap: 'wrap',
  alignItems: 'center',
  gap: spacing.gap.xs,
};

const addPanelButtonStyle: StyleWithVars = {
  backgroundColor: colors.bg.surface,
  color: colors.text.secondary,
  border: `1px solid ${colors.border.DEFAULT}`,
  borderRadius: borderRadius.md,
  fontSize: typography.fontSize['xs'],
  lineHeight: typography.lineHeight.tight,
  padding: `${spacing.padding.xs} ${spacing.gap.sm}`,
  '--toolbar-button-hover-bg': colors.bg.hover,
  '--toolbar-button-hover-border': colors.border.hover,
};

const resetButtonStyle: StyleWithVars = {
  backgroundColor: colors.bg.active,
  color: colors.accent.muted,
  border: `1px solid ${colors.border.DEFAULT}`,
  borderRadius: borderRadius.md,
  fontSize: typography.fontSize['xs'],
  lineHeight: typography.lineHeight.tight,
  padding: `${spacing.padding.xs} ${spacing.gap.sm}`,
  '--toolbar-button-hover-bg': colors.bg.hover,
  '--toolbar-button-hover-border': colors.accent.muted,
};

function hasBreakpointLayout(layouts: Layouts, breakpoint: BreakpointKey): boolean {
  return Object.prototype.hasOwnProperty.call(layouts, breakpoint);
}

function cloneLayouts(layouts: Layouts = {} as Layouts): Layouts {
  const next: Layouts = {};
  if (!layouts) {
    return next;
  }

  Object.keys(layouts).forEach(key => {
    const source = (layouts as Record<string, Layout[] | undefined>)[key];
    if (!source) {
      return;
    }
    next[key] = source.map(item => ({ ...item }));
  });
  return next;
}

function normalizeLayouts(layouts: Layouts): Layouts {
  return cloneLayouts(layouts || ({} as Layouts));
}

function getLayoutList(layouts: Layouts, breakpoint: BreakpointKey): Layout[] {
  return layouts[breakpoint] ?? [];
}

function filterLayoutsByAllowedPanels(layouts: Layouts, allowedPanels: ReadonlySet<string>): Layouts {
  const next: Layouts = {};
  BREAKPOINT_ORDER.forEach(bp => {
    if (!hasBreakpointLayout(layouts, bp)) {
      return;
    }
    const list = layouts[bp] ?? [];
    const filtered = list.filter(item => allowedPanels.has(item.i));
    if (filtered.length === 0) {
      return;
    }
    next[bp] = filtered;
  });
  return next;
}

function filterCollapsedPanelsByAllowedPanels(
  collapsedPanels: Set<string>,
  allowedPanels: ReadonlySet<string>
): Set<string> {
  return new Set(Array.from(collapsedPanels).filter(panelId => allowedPanels.has(panelId)));
}

function DesktopDashboardLayout({
  preset = 'default',
  allowedPanels = ALL_PANELS,
}: {
  preset?: keyof typeof PRESETS | string;
  allowedPanels?: readonly PanelId[];
}) {
  const presetKey = String(preset);
  const allowedPanelIds = useMemo(() => {
    const normalized = Array.from(new Set(allowedPanels));
    return normalized.filter((panelId): panelId is PanelId =>
      Object.prototype.hasOwnProperty.call(PANEL_REGISTRY, panelId)
    );
  }, [allowedPanels]);
  const allowedPanelIdSet = useMemo(() => new Set<string>(allowedPanelIds), [allowedPanelIds]);
  const lastSavedLayoutsRef = useRef<string>('');
  const scrollContainerRef = useRef<HTMLDivElement | null>(null);
  const [layouts, setLayouts] = useState<Layouts>(() => {
    const initial = filterLayoutsByAllowedPanels(normalizeLayouts(loadLayout(presetKey)), allowedPanelIdSet);
    lastSavedLayoutsRef.current = JSON.stringify(initial);
    return initial;
  });
  const [collapsedPanels, setCollapsedPanels] = useState<Set<string>>(() =>
    filterCollapsedPanelsByAllowedPanels(loadCollapsedPanels(), allowedPanelIdSet)
  );
  const [activeBreakpoint, setActiveBreakpoint] = useState<BreakpointKey>('lg');
  const [gridWidth, setGridWidth] = useState(() => {
    if (typeof window === 'undefined') {
      return 1200;
    }
    return Math.max(1, window.innerWidth);
  });
  const latestLayoutsRef = useRef<Layouts>(layouts);

  useEffect(() => {
    latestLayoutsRef.current = layouts;
  }, [layouts]);

  const orderedPanels = useMemo(() => {
    const activeLayout = getLayoutList(layouts, activeBreakpoint);
    const fallbackLayout = activeLayout.length > 0 ? activeLayout : getLayoutList(layouts, 'lg');
    return fallbackLayout.map(item => item.i);
  }, [layouts, activeBreakpoint]);

  const defaultLayouts = useMemo(
    () => filterLayoutsByAllowedPanels(normalizeLayouts(createLayoutsFromPreset(presetKey)), allowedPanelIdSet),
    [presetKey, allowedPanelIdSet]
  );

  const activePanelIds = useMemo(() => new Set(orderedPanels), [orderedPanels]);
  const availablePanels = allowedPanelIds.filter(id => !activePanelIds.has(id));

  useEffect(() => {
    const filteredLayouts = filterLayoutsByAllowedPanels(layouts, allowedPanelIdSet);
    const serializedFilteredLayouts = JSON.stringify(filteredLayouts);
    if (serializedFilteredLayouts !== JSON.stringify(layouts)) {
      setLayouts(filteredLayouts);
      lastSavedLayoutsRef.current = serializedFilteredLayouts;
      saveLayout(presetKey, filteredLayouts);
    }

    const filteredCollapsedPanels = filterCollapsedPanelsByAllowedPanels(collapsedPanels, allowedPanelIdSet);
    if (filteredCollapsedPanels.size !== collapsedPanels.size) {
      setCollapsedPanels(filteredCollapsedPanels);
      saveCollapsedPanels(filteredCollapsedPanels);
    }
  }, [allowedPanelIdSet, collapsedPanels, layouts, presetKey]);

  const adjustedLayouts = useMemo(() => {
    const adjusted: Layouts = {};
    BREAKPOINT_ORDER.forEach(bp => {
      if (!hasBreakpointLayout(layouts, bp)) {
        return;
      }

      adjusted[bp] = getLayoutList(layouts, bp).map(item => {
        const Component = PANEL_REGISTRY[item.i as PanelId];
        const defaultSize = (Component as any)?.defaultSize || {};
        const constraints = {
          minW: defaultSize.minW,
          maxW: defaultSize.maxW,
          minH: defaultSize.minH,
          maxH: defaultSize.maxH,
        };

        if (collapsedPanels.has(item.i)) {
          return { ...item, ...constraints, h: 1, minH: 1 };
        }

        const minHeight = defaultSize.minH ?? defaultSize.h ?? 2;
        const enforcedHeight = Math.max(item.h ?? minHeight, minHeight);
        return { ...item, ...constraints, h: enforcedHeight };
      });
    });
    return adjusted;
  }, [layouts, collapsedPanels]);

  const persistLayouts = (nextLayouts: Layouts, options?: { skipStateUpdate?: boolean }) => {
    const normalized = filterLayoutsByAllowedPanels(normalizeLayouts(nextLayouts), allowedPanelIdSet);
    if (!options?.skipStateUpdate) {
      setLayouts(normalized);
    }
    const serialized = JSON.stringify(normalized);
    if (serialized === lastSavedLayoutsRef.current) {
      return;
    }
    lastSavedLayoutsRef.current = serialized;
    saveLayout(presetKey, normalized);
  };

  const mergeLayoutsWithCollapsedHeights = (incoming: Layouts): Layouts => {
    const merged: Layouts = {};

    BREAKPOINT_ORDER.forEach(bp => {
      const baseList = getLayoutList(layouts, bp);
      const nextList = incoming[bp];
      if (!nextList) {
        merged[bp] = baseList.map(item => ({ ...item }));
        return;
      }

      merged[bp] = nextList.map(item => {
        if (!collapsedPanels.has(item.i)) {
          return { ...item };
        }
        const original = baseList.find(entry => entry.i === item.i);
        return {
          ...item,
          h: original?.h ?? item.h,
        };
      });
    });

    return merged;
  };

  const handleLayoutChange = (_current: Layout[], allLayouts: Layouts) => {
    const normalized = normalizeLayouts(allLayouts);
    const merged = filterLayoutsByAllowedPanels(
      mergeLayoutsWithCollapsedHeights(normalized),
      allowedPanelIdSet
    );
    latestLayoutsRef.current = merged;
    setLayouts(merged);
  };

  const handleLayoutStop = () => {
    persistLayouts(latestLayoutsRef.current, { skipStateUpdate: true });
  };

  const handleRemovePanel = (panelId: string) => {
    const nextLayouts = cloneLayouts(layouts);
    BREAKPOINT_ORDER.forEach(bp => {
      if (!hasBreakpointLayout(nextLayouts, bp)) {
        return;
      }
      nextLayouts[bp] = nextLayouts[bp]?.filter(item => item.i !== panelId) ?? [];
    });
    persistLayouts(nextLayouts);

    setCollapsedPanels(prev => {
      const next = new Set(prev);
      next.delete(panelId);
      saveCollapsedPanels(next);
      return next;
    });
  };

  const handleAddPanel = (panelId: PanelId) => {
    if (activePanelIds.has(panelId) || !allowedPanelIdSet.has(panelId)) return;

    const Component = PANEL_REGISTRY[panelId];
    const defaultSize = (Component as any).defaultSize || { w: 12, h: 4 };

    const nextLayouts = cloneLayouts(layouts);
    BREAKPOINT_ORDER.forEach(bp => {
      const cols = COLS[bp];
      const list = nextLayouts[bp] ?? [];
      const maxY = list.reduce((max, item) => Math.max(max, item.y + item.h), 0);
      list.push({
        i: panelId,
        x: 0,
        y: maxY,
        w: Math.min(defaultSize.w, cols),
        h: defaultSize.h,
      });
      nextLayouts[bp] = list;
    });

    persistLayouts(nextLayouts);
  };

  const handlePanelCollapsedChange = (panelId: string, collapsed: boolean) => {
    setCollapsedPanels(prev => {
      const next = new Set(prev);
      if (collapsed) {
        next.add(panelId);
      } else {
        next.delete(panelId);
      }
      saveCollapsedPanels(next);
      return next;
    });
  };

  const handleBreakpointChange = (breakpoint: BreakpointKey) => {
    setActiveBreakpoint(breakpoint);
  };

  const handleResetLayout = () => {
    const normalized = cloneLayouts(defaultLayouts);
    setLayouts(normalized);
    saveLayout(presetKey, normalized);
    const empty = new Set<string>();
    setCollapsedPanels(empty);
    saveCollapsedPanels(empty);
  };

  // Track the grid container width so ResponsiveGridLayout always receives
  // an accurate width, even when nested inside flex/overflow wrappers.
  useEffect(() => {
    if (typeof window === 'undefined') {
      return;
    }

    const node = scrollContainerRef.current;
    if (!node) {
      return;
    }

    const updateWidth = (nextWidth: number) => {
      const normalized = Math.max(1, Math.floor(nextWidth));
      setGridWidth(prev => (prev === normalized ? prev : normalized));
    };

    let resizeObserver: ResizeObserver | null = null;
    let rafId: number | null = null;

    const scheduleMeasure = () => {
      if (rafId !== null) {
        window.cancelAnimationFrame(rafId);
      }
      rafId = window.requestAnimationFrame(() => {
        updateWidth(node.getBoundingClientRect().width);
      });
    };

    const ObserverCtor = (window as typeof window & { ResizeObserver?: typeof ResizeObserver }).ResizeObserver;

    if (typeof ObserverCtor !== 'undefined') {
      resizeObserver = new ObserverCtor(entries => {
        if (!entries.length) return;
        updateWidth(entries[0].contentRect.width);
      });
      resizeObserver.observe(node);
      // Ensure we capture the initial width immediately.
      updateWidth(node.getBoundingClientRect().width);
    } else {
      scheduleMeasure();
      window.addEventListener('resize', scheduleMeasure);
    }

    return () => {
      if (resizeObserver) {
        resizeObserver.disconnect();
      } else {
        window.removeEventListener('resize', scheduleMeasure);
      }
      if (rafId !== null) {
        window.cancelAnimationFrame(rafId);
      }
    };
  }, []);

  const renderPanel = (panelId: string) => {
    if (!allowedPanelIdSet.has(panelId)) {
      return null;
    }
    const Component = PANEL_REGISTRY[panelId as PanelId];
    if (!Component) {
      if (import.meta.env?.DEV) {
        console.warn(`[dashboard] Unknown panel: ${panelId}`);
      }
      return null;
    }

    const layoutEntry = getLayoutList(layouts, 'lg').find(item => item.i === panelId);
    const fullWidth = layoutEntry ? layoutEntry.w === COLS.lg : false;
    const isCollapsed = collapsedPanels.has(panelId);

    return (
      <div key={panelId}>
        <Component
          title={(Component as any).displayName || panelId}
          onRemove={() => handleRemovePanel(panelId)}
          onCollapsedChange={(collapsed: boolean) => handlePanelCollapsedChange(panelId, collapsed)}
          fullWidth={fullWidth}
          collapsed={isCollapsed}
        />
      </div>
    );
  };

  return (
    <div className="page-shell h-full dashboard-root flex flex-col">
      <div className="flex flex-col h-full w-full min-h-0">
      {(availablePanels.length > 0 || orderedPanels.length > 0) && (
        <div className="flex flex-wrap items-center" style={toolbarStyle}>
          {availablePanels.length > 0 && (
            <div className="flex flex-wrap items-center" style={{ gap: spacing.gap.xs }}>
              <span style={toolbarLabelStyle}>Add:</span>
              {availablePanels.map(panelId => {
                const Component = PANEL_REGISTRY[panelId];
                const displayName = (Component as any).displayName || panelId;
                return (
                  <button
                    key={panelId}
                    onClick={() => handleAddPanel(panelId)}
                    className="dashboard-toolbar-button"
                    style={addPanelButtonStyle}
                    title={`Add ${displayName}`}
                    aria-label={`Add ${displayName} panel`}
                  >
                    + {displayName}
                  </button>
                );
              })}
            </div>
          )}
          <div className="flex flex-wrap items-center" style={toolbarActionsStyle}>
            <button
              onClick={() => {
                if (window.confirm('Reset dashboard layout to preset defaults?')) {
                  handleResetLayout();
                }
              }}
              className="dashboard-toolbar-button"
              style={resetButtonStyle}
              aria-label="Reset dashboard layout"
            >
              Reset Layout
            </button>
          </div>
        </div>
      )}

      <div
        ref={scrollContainerRef}
        className="flex-1 overflow-auto pb-32"
        style={{ minWidth: 0, minHeight: 0, overflowX: 'auto', overflowY: 'auto' }}
      >
        <section aria-label="Dashboard panels" className="h-full">
          <ResponsiveGridLayout
            className="layout"
            layouts={adjustedLayouts}
            breakpoints={BREAKPOINTS}
            cols={COLS}
            width={Math.max(1, gridWidth)}
            rowHeight={60}
            margin={[4, 4]}
            containerPadding={[0, 4]}
            compactType="vertical"
            isBounded
            preventCollision={PREVENT_COLLISION}
            onLayoutChange={handleLayoutChange}
            onDragStop={handleLayoutStop}
            onResizeStop={handleLayoutStop}
            onBreakpointChange={(bp) => handleBreakpointChange(bp as BreakpointKey)}
            draggableHandle=".drag-handle"
            resizeHandles={['se', 'e']}
          >
            {orderedPanels.map(panelId => renderPanel(panelId))}
          </ResponsiveGridLayout>
          <div
            data-testid="dashboard-bottom-spacer"
            aria-hidden="true"
            className="pointer-events-none w-full shrink-0"
            style={{ height: '160px', background: '#050505' }}
          />
        </section>
      </div>
      </div>
    </div>
  );
}

export function DashboardLayout({
  preset = 'default',
  allowedPanels,
}: {
  preset?: keyof typeof PRESETS | string;
  allowedPanels?: readonly PanelId[];
}) {
  const isMobile = useIsMobile();
  if (isMobile) {
    return <MobileDashboard />;
  }
  return <DesktopDashboardLayout preset={preset} allowedPanels={allowedPanels} />;
}
