/**
 * PanelHeader - Standardized panel header component
 *
 * Provides consistent header UI for all Fluxboard panels with:
 * - Title with optional freshness indicator
 * - Standard action buttons (refresh, expand, full-page, remove)
 * - Custom action slot for panel-specific controls
 * - Token-based styling for consistency
 *
 * @example
 * ```tsx
 * <PanelHeader
 *   title="Balances"
 *   onRefresh={handleRefresh}
 *   lastUpdate={Date.now()}
 *   staleThresholdMs={10000}
 *   actions={<Button onClick={handleExport}>Export CSV</Button>}
 * />
 * ```
 */

import { useNavigate } from 'react-router-dom';
import { RefreshCw, Maximize2, Plus, Minus, X } from 'lucide-react';
import { FreshnessIndicator } from './FreshnessIndicator';
import { IconButton } from '../ui';
import { colors, spacing, typography } from '@/lib/tokens';
import { cn } from '@/lib/utils';
import { useDensityMode } from '@/hooks/useMobileLayout';

export interface PanelHeaderProps {
  /** Panel title displayed on the left */
  title: string;
  /** Callback when refresh button is clicked */
  onRefresh?: () => void;
  /** Callback when collapse/expand button is clicked */
  onToggleCollapse?: () => void;
  /** Current collapse state */
  collapsed?: boolean;
  /** URL for full-page view (enables full-page button) */
  fullPageUrl?: string;
  /** Callback when remove button is clicked */
  onRemove?: () => void;
  /** Unix timestamp in milliseconds for freshness indicator */
  lastUpdate?: number;
  /** Threshold in milliseconds for considering data stale */
  staleThresholdMs?: number;
  /** Custom action buttons inline with title (e.g., speaker icon) */
  titleActions?: React.ReactNode;
  /** Custom action buttons (e.g., Export, filters) */
  actions?: React.ReactNode;
  /** Additional CSS class names */
  className?: string;
  /** Whether refresh is in progress (shows spinner) */
  refreshing?: boolean;
  /** Density override; defaults to layout density */
  density?: 'mobile' | 'desktop';
}

export function PanelHeader({
  title,
  onRefresh,
  onToggleCollapse,
  collapsed,
  fullPageUrl,
  onRemove,
  lastUpdate,
  staleThresholdMs,
  titleActions,
  actions,
  className,
  refreshing = false,
  density,
}: PanelHeaderProps) {
  const resolvedDensity = useDensityMode(density);
  let navigate: ReturnType<typeof useNavigate> | null = null;
  try {
    navigate = useNavigate();
  } catch {
    navigate = null;
  }

  const handleFullPage = (e: React.MouseEvent) => {
    e.stopPropagation();
    if (fullPageUrl) {
      navigate?.(fullPageUrl);
    }
  };

  const handleRefresh = (e: React.MouseEvent) => {
    e.stopPropagation();
    onRefresh?.();
  };

  const handleToggleCollapse = (e: React.MouseEvent) => {
    e.stopPropagation();
    onToggleCollapse?.();
  };

  const handleRemove = (e: React.MouseEvent) => {
    e.stopPropagation();
    onRemove?.();
  };

  const headerHeight = resolvedDensity === 'mobile' ? '48px' : spacing.row.header;
  const gap = resolvedDensity === 'mobile' ? spacing.gap.sm : spacing.gap.xs;

  return (
    <div
      className={cn("flex items-center justify-between", className)}
      style={{
        padding: '10px 12px',
        backgroundColor: colors.bg.surface,
        borderBottom: `1px solid ${colors.border.DEFAULT}`,
        minHeight: headerHeight,
        boxShadow: 'none',
      }}
    >
      {/* Left: Title, freshness indicator, and inline actions */}
      <div
        className="flex items-center flex-1 drag-handle cursor-move"
        style={{ gap }}
        title="Drag to move panel"
        aria-label="Drag handle"
      >
        <h3
          style={{
            fontSize: resolvedDensity === 'mobile' ? typography.fontSize.lg : typography.fontSize.md,
            fontWeight: typography.fontWeight.semibold,
            color: colors.text.primary,
            letterSpacing: '-0.01em',
          }}
        >
          {title}
        </h3>
        {lastUpdate !== undefined && (
          <FreshnessIndicator lastUpdate={lastUpdate} staleThresholdMs={staleThresholdMs} />
        )}
        {titleActions && (
          <div className="flex items-center" style={{ gap: spacing.gap.xs }}>
            {titleActions}
          </div>
        )}
      </div>

      {/* Right: Custom actions + standard buttons */}
      <div className="flex items-center" style={{ gap }}>
        {/* Custom actions slot (e.g., Export button) */}
        {actions && (
          <>
            {actions}
            {/* Separator between custom actions and standard buttons */}
            {(onRefresh || fullPageUrl || onToggleCollapse || onRemove) && (
              <div
                style={{
                  width: '1px',
                  height: '20px',
                  backgroundColor: colors.border.DEFAULT,
                  marginLeft: spacing.gap.xs,
                  marginRight: spacing.gap.xs,
                }}
              />
            )}
          </>
        )}

        {/* Standard action buttons */}
        {onRefresh && (
          <IconButton
            variant={'secondary' as const}
            size={'xs' as const}
            onClick={handleRefresh}
            aria-label="Refresh"
            title="Refresh"
            disabled={refreshing}
            density={resolvedDensity}
          >
            <RefreshCw className={cn("w-3 h-3", refreshing && "animate-spin")} />
          </IconButton>
        )}
        {fullPageUrl && (
          <IconButton
            variant={'secondary' as const}
            size={'xs' as const}
            onClick={handleFullPage}
            aria-label="Open Full Page"
            title="Open Full Page"
            density={resolvedDensity}
          >
            <Maximize2 className="w-3 h-3" />
          </IconButton>
        )}
        {onToggleCollapse && (
          <IconButton
            variant={'secondary' as const}
            size={'xs' as const}
            onClick={handleToggleCollapse}
            aria-label={collapsed ? 'Expand' : 'Collapse'}
            title={collapsed ? 'Expand' : 'Collapse'}
            aria-expanded={!collapsed}
            density={resolvedDensity}
          >
            {collapsed ? <Plus className="w-3 h-3" /> : <Minus className="w-3 h-3" />}
          </IconButton>
        )}
        {onRemove && (
          <IconButton
            variant={'danger' as const}
            size={'xs' as const}
            onClick={handleRemove}
            aria-label="Remove"
            title="Remove"
            density={resolvedDensity}
          >
            <X className="w-3 h-3" />
          </IconButton>
        )}
      </div>
    </div>
  );
}
