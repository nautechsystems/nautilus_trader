// Panel wrapper with header and collapse functionality

import { createContext, useContext, useEffect, useMemo, useState, type ReactNode } from 'react';
import { motion } from 'framer-motion';
import { PanelHeader } from '../shared/PanelHeader';
import { colors, borderRadius } from '@/lib/tokens';

type PanelHeaderSlotsContextValue = {
  setTitleActions: (actions: ReactNode | null) => void;
  setActions: (actions: ReactNode | null) => void;
};

const PanelHeaderSlotsContext = createContext<PanelHeaderSlotsContextValue | null>(null);

export function usePanelHeaderSlots() {
  return useContext(PanelHeaderSlotsContext);
}

export function PanelWrapper({
  title,
  children,
  onRefresh,
  fullPageUrl,
  onRemove,
  collapsible = true,
  onCollapsedChange,
  lastUpdate,
  staleThresholdMs,
  density = 'compact',
  fullWidth = false,
  collapsed: collapsedProp,
}: {
  title: string;
  children: ReactNode;
  onRefresh?: () => void;
  fullPageUrl?: string;
  onRemove?: () => void;
  collapsible?: boolean;
  onCollapsedChange?: (collapsed: boolean) => void;
  lastUpdate?: number;  // Unix timestamp in milliseconds
  staleThresholdMs?: number;  // Threshold for stale data
  density?: 'compact' | 'relaxed';
  fullWidth?: boolean;  // When true, panel spans full width without side columns
  collapsed?: boolean;
}) {
  const isControlled = typeof collapsedProp === 'boolean';
  const [internalCollapsed, setInternalCollapsed] = useState<boolean>(collapsedProp ?? false);
  const [titleActions, setTitleActions] = useState<ReactNode | null>(null);
  const [actions, setActions] = useState<ReactNode | null>(null);
  const [reducedMotion, setReducedMotion] = useState(false);

  useEffect(() => {
    if (isControlled) {
      setInternalCollapsed(collapsedProp ?? false);
    }
  }, [collapsedProp, isControlled]);

  const collapsed = isControlled ? Boolean(collapsedProp) : internalCollapsed;

  useEffect(() => {
    try {
      const m = window.matchMedia?.('(prefers-reduced-motion: reduce)');
      setReducedMotion(!!m?.matches);
      const handler = (e: MediaQueryListEvent) => setReducedMotion(e.matches);
      m?.addEventListener?.('change', handler);
      return () => m?.removeEventListener?.('change', handler);
    } catch {
      /* noop */
    }
  }, []);



  const handleToggleCollapse = () => {
    const newCollapsed = !collapsed;
    if (!isControlled) {
      setInternalCollapsed(newCollapsed);
    }
    onCollapsedChange?.(newCollapsed);
  };

  const contextValue = useMemo<PanelHeaderSlotsContextValue>(
    () => ({
      setTitleActions,
      setActions,
    }),
    [setTitleActions, setActions]
  );

  return (
    <PanelHeaderSlotsContext.Provider value={contextValue}>
      <motion.div
        className={`dashboard-panel flex flex-col overflow-hidden ${collapsed ? 'h-auto' : 'h-full'}`}
        data-panel-title={title}
        initial={reducedMotion ? undefined : { opacity: 0, y: 2 }}
        animate={reducedMotion ? undefined : { opacity: 1, y: 0 }}
        transition={reducedMotion ? undefined : { duration: 0.15, ease: 'easeOut' }}
        style={{
          height: '100%',
          backgroundColor: colors.bg.surface,
          border: `1px solid ${colors.border.DEFAULT}`,
          borderRadius: borderRadius.DEFAULT,
        }}
      >
        <PanelHeader
          title={title}
          onRefresh={onRefresh}
          onToggleCollapse={collapsible ? handleToggleCollapse : undefined}
          collapsed={collapsed}
          fullPageUrl={fullPageUrl}
          onRemove={onRemove}
          lastUpdate={lastUpdate}
          staleThresholdMs={staleThresholdMs}
          titleActions={titleActions}
          actions={actions}
        />
        {!collapsed && (
          <div
            className="flex flex-1 flex-col p-0 overflow-hidden"
            style={{ minHeight: 0 }}
            data-testid="panel-body"
          >
            {children}
          </div>
        )}
      </motion.div>
    </PanelHeaderSlotsContext.Provider>
  );
}
