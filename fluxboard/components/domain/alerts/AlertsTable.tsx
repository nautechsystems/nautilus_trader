/**
 * AlertsTable Component
 *
 * Direct table rendering with severity filtering, auto-dismiss logic,
 * and inline expandable JSON details. Uses design tokens for severity color bands.
 */

import { useMemo, useState, Fragment, useRef, useEffect } from 'react';
import { Badge } from '@/components/ui/badge';
import { TimeAgo } from '@/components/shared/TimeAgo';
import { IconButton } from '@/components/ui/button/IconButton';
import { X } from 'lucide-react';
import type { Alert, AlertLevel } from '@/types';
import { severity, colors, typography, getDensityStyles } from '@/lib/tokens';
import { ALERT_AUTO_DISMISS } from '@/constants';
import { StatusPill } from '@/components/shared/StatusPill';
import type { StatusKind } from '@/components/shared/status';
import { useMobileLayout } from '@/hooks/useMobileLayout';
import { cn } from '@/lib/utils';

export interface AlertsTableProps {
  /**
   * Alert rows to display
   */
  alerts: Alert[];

  /**
   * Loading state
   */
  loading?: boolean;

  /**
   * Currently dismissed alert IDs
   */
  dismissedIds: Set<string>;

  /**
   * Active level filter
   */
  levelFilter: AlertLevel | 'ALL';

  /**
   * Dismiss alert handler
   */
  onDismiss: (id: string) => void;

  /**
   * Row click handler (expand details)
   */
  onRowClick: (alert: Alert) => void;

  /**
   * Currently expanded alert ID
   */
  expandedAlertId: string | null;

  /**
   * Dense mode (compact spacing)
   */
  dense?: boolean;
}

// TimeAgo handles live updates and formatting; no local ticker needed here.

/**
 * Map alert level to StatusPill semantic status.
 */
function getSeverityStatus(level: string): StatusKind {
  if (level === 'CRITICAL' || level === 'ERROR') return 'critical';
  if (level === 'WARNING') return 'warning';
  return 'info';
}

/**
 * Get severity color for left border
 */
function getSeverityBorderColor(level: string): string {
  if (level === 'CRITICAL' || level === 'ERROR') return severity.critical.color;
  if (level === 'WARNING') return severity.warning.color;
  return severity.info.color;
}

/**
 * Summarize alert for display
 */
function summarizeAlert(alert: Alert): string {
  const context = (alert as any).context as Record<string, unknown> | undefined;
  const details = (alert as any).details as Record<string, unknown> | undefined;
  const summaryCandidates: Array<unknown> = [
    (alert as any).summary,
    (alert as any).subtitle,
    context?.summary,
    context?.description,
    details?.summary,
    details?.description,
    context?.error,
    context?.status,
    context?.reason,
    details?.error,
    details?.status
  ];

  for (const candidate of summaryCandidates) {
    if (typeof candidate === 'string' && candidate.trim().length > 0) {
      return candidate.trim();
    }
  }

  if (typeof context?.edge_bps === 'number') {
    return `Edge ${context.edge_bps.toFixed(2)} bps`;
  }
  if (typeof (alert as any).message === 'string') {
    return (alert as any).message;
  }
  return '-';
}

export function AlertsTable({
  alerts,
  loading = false,
  dismissedIds,
  levelFilter,
  onDismiss,
  onRowClick,
  expandedAlertId,
  dense = false,
}: AlertsTableProps) {
  // No per-row tickers; TimeAgo renders its own live label.

  // Auto-dismiss logic (track scheduled timers to prevent duplicates)
  const scheduledTimersRef = useRef<Map<string, NodeJS.Timeout>>(new Map());
  const { isMobile, density } = useMobileLayout();

  useEffect(() => {
    const scheduledTimers = scheduledTimersRef.current;

    // Only set timers for currently visible (filtered) rows to reduce timer churn
    const visible = alerts.filter(
      (a) => !dismissedIds.has(a.id) && (levelFilter === 'ALL' || (a.severity || a.level) === levelFilter)
    );

    // Schedule new timers only for alerts that don't have one yet
    visible.forEach((alert) => {
      if (scheduledTimers.has(alert.id)) return; // Already scheduled

      const timeout = ALERT_AUTO_DISMISS[(alert.severity || alert.level) as keyof typeof ALERT_AUTO_DISMISS];
      if (timeout > 0) {
        const timer = setTimeout(() => {
          scheduledTimers.delete(alert.id);
          onDismiss(alert.id);
        }, timeout);
        scheduledTimers.set(alert.id, timer);
      }
    });

    // Clean up timers for alerts that are no longer visible
    const visibleIds = new Set(visible.map(a => a.id));
    for (const [id, timer] of scheduledTimers.entries()) {
      if (!visibleIds.has(id)) {
        clearTimeout(timer);
        scheduledTimers.delete(id);
      }
    }

    return () => {
      scheduledTimers.forEach(timer => clearTimeout(timer));
      scheduledTimers.clear();
    };
  }, [alerts, levelFilter, onDismiss, dismissedIds]);

  // Filter and sort rows
  const filteredAlerts = useMemo(
    () => {
      const filtered = alerts.filter(
        (a) => !dismissedIds.has(a.id) && (levelFilter === 'ALL' || (a.severity || a.level) === levelFilter)
      );
      // Sort by timestamp descending (newest first)
      return filtered.sort((a, b) => {
        const tsA = a.ts || a.timestamp || 0;
        const tsB = b.ts || b.timestamp || 0;
        return tsB - tsA;
      });
    },
    [alerts, dismissedIds, levelFilter]
  );

  if (loading) {
    return (
      <div className="w-full h-32 flex items-center justify-center text-sm" style={{ color: colors.text.muted }}>
        Loading alerts...
      </div>
    );
  }

  if (filteredAlerts.length === 0) {
    return (
      <div className="w-full h-32 flex items-center justify-center text-sm" style={{ color: colors.text.muted }}>
        No alerts
      </div>
    );
  }

  if (isMobile) {
    return (
      <div className="flex flex-col gap-3">
        {filteredAlerts.map((alert) => (
          <AlertCard
            key={alert.id}
            alert={alert}
            onDismiss={onDismiss}
            onRowClick={onRowClick}
          />
        ))}
      </div>
    );
  }

  const rowPadding = dense ? 'py-1 px-2' : 'py-2 px-3';
  const bodyTypography = getDensityStyles(dense, density);
  const bodyTypographyStyle = {
    fontSize: bodyTypography.fontSize,
    fontWeight: typography.fontWeight.normal,
  } as const;
  const headerBaseClass = `${rowPadding} text-left text-xs font-semibold uppercase tracking-[0.04em]`;

  return (
    <div className="w-full overflow-auto">
      <table className="w-full border-collapse">
        <thead className="sticky top-0 z-10 backdrop-blur" style={{ backgroundColor: colors.bg.base, borderBottomColor: colors.border.DEFAULT, borderBottomWidth: '1px' }}>
          <tr>
            <th className={headerBaseClass} style={{ color: colors.text.muted }}>Age</th>
            <th className={headerBaseClass} style={{ color: colors.text.muted }}>Severity</th>
            <th className={headerBaseClass} style={{ color: colors.text.muted }}>Title</th>
            <th className={headerBaseClass} style={{ color: colors.text.muted }}>Strategy</th>
            <th className={headerBaseClass} style={{ color: colors.text.muted }}>Summary</th>
            <th className={`${headerBaseClass} text-right`} style={{ color: colors.text.muted }}></th>
          </tr>
        </thead>
        <tbody>
          {filteredAlerts.map((alert) => {
            const isExpanded = expandedAlertId === alert.id;
            const ts = alert.ts || alert.timestamp || 0;
            const level = alert.severity || alert.level || 'INFO';
            const borderColor = getSeverityBorderColor(level);

            return (
              <Fragment key={alert.id}>
                {/* Main alert row */}
                <tr
                  onClick={() => onRowClick(alert)}
                  className={cn(
                    'cursor-pointer transition-colors hover:bg-bg-hover border-b border-border',
                    isExpanded && 'alert-row--expanded bg-bg-active'
                  )}
                  data-expanded={isExpanded ? 'true' : 'false'}
                  aria-expanded={isExpanded}
                  style={{
                    borderBottomColor: colors.border.DEFAULT,
                    borderBottomWidth: '1px',
                    boxShadow: isExpanded ? `inset 3px 0 0 ${colors.accent.DEFAULT}` : undefined,
                  }}
                >
                  {/* Age + Expansion indicator */}
                  <td className={rowPadding} style={bodyTypographyStyle}>
                    <div
                      className="flex items-center gap-2"
                      style={{ borderLeft: `4px solid ${borderColor}`, paddingLeft: '8px' }}
                    >
                      <TimeAgo
                        timestamp={ts}
                        className="text-text-secondary"
                      />
                    </div>
                  </td>

                  {/* Severity */}
                  <td className={rowPadding} style={bodyTypographyStyle}>
                    <StatusPill
                      status={getSeverityStatus(level)}
                      label={level}
                      size="xs"
                      tone="subtle"
                    />
                  </td>

                  {/* Title */}
                  <td
                    className={`${rowPadding} whitespace-nowrap overflow-hidden text-ellipsis max-w-xs`}
                    style={{ ...bodyTypographyStyle, color: colors.text.primary }}
                  >
                    {alert.title || alert.message || ''}
                  </td>

                  {/* Strategy */}
                  <td className={`${rowPadding} font-mono`} style={{ ...bodyTypographyStyle, color: colors.text.secondary }}>
                    {alert.strategy_id || '-'}
                  </td>

                  {/* Summary */}
                  <td
                    className={`${rowPadding} whitespace-nowrap overflow-hidden text-ellipsis max-w-md`}
                    style={{ ...bodyTypographyStyle, color: colors.text.muted }}
                  >
                    {summarizeAlert(alert)}
                  </td>

                  {/* Actions */}
                  <td className={`${rowPadding} text-right`} style={bodyTypographyStyle}>
                    <div className="inline-flex items-center gap-2" onClick={(e) => e.stopPropagation()}>
                      <button
                        type="button"
                        className={cn(
                          'inline-flex items-center gap-1 rounded border px-2 py-1 text-[11px] font-medium transition-colors',
                          isExpanded
                            ? 'bg-bg-active border-border-hover text-text-primary'
                            : 'border-border text-text-secondary hover:bg-bg-hover hover:text-text-primary'
                        )}
                        aria-label={`${isExpanded ? 'Collapse' : 'Expand'} details for alert ${alert.id}`}
                        aria-expanded={isExpanded}
                        onClick={(e) => {
                          e.stopPropagation();
                          onRowClick(alert);
                        }}
                      >
                        <span aria-hidden>{isExpanded ? '▼' : '▶'}</span>
                        <span>{isExpanded ? 'Collapse' : 'Expand'}</span>
                      </button>
                      <IconButton
                        variant="ghost"
                        size="xs"
                        aria-label={`Dismiss alert ${alert.id}`}
                        onClick={(e) => {
                          e.stopPropagation();
                          onDismiss(alert.id);
                        }}
                      >
                        <X className="h-3 w-3" style={{ color: colors.text.muted }} />
                      </IconButton>
                    </div>
                  </td>
                </tr>

                {/* Inline expansion row */}
                {isExpanded && (
                  <tr style={{ backgroundColor: colors.bg.surface }}>
                    <td colSpan={6} className="p-4 border-b" style={{ borderColor: colors.border.DEFAULT }}>
                      <InlineAlertDetails alert={alert} />
                    </td>
                  </tr>
                )}
              </Fragment>
            );
          })}
        </tbody>
      </table>
    </div>
  );
}

interface AlertCardProps {
  alert: Alert;
  onDismiss: (id: string) => void;
  onRowClick: (alert: Alert) => void;
}

const AlertCard = ({ alert, onDismiss, onRowClick }: AlertCardProps) => {
  const level = alert.severity || alert.level || 'INFO';
  const summary = summarizeAlert(alert);
  const ts = alert.ts || alert.timestamp || 0;

  return (
    <div className="rounded-xl border p-3 flex flex-col gap-3" style={{ borderColor: colors.border.DEFAULT, backgroundColor: colors.bg.surface }}>
      <div className="flex items-start justify-between gap-2">
        <div className="flex flex-col gap-1">
          <Badge variant={level === 'CRITICAL' || level === 'ERROR' ? 'danger' : level === 'WARNING' ? 'warning' : 'neutral'} size="xs">
            {level}
          </Badge>
          <div className="text-sm font-medium" style={{ color: colors.text.primary }}>{alert.title || alert.message || 'Alert'}</div>
          <div className="text-xs" style={{ color: colors.text.muted }}>{summary}</div>
        </div>
        <IconButton
          variant="ghost"
          size="xs"
          density="mobile"
          aria-label={`Dismiss alert ${alert.id}`}
          onClick={(e) => {
            e.stopPropagation();
            onDismiss(alert.id);
          }}
        >
          <X className="h-3 w-3" style={{ color: colors.text.muted }} />
        </IconButton>
      </div>
      <div className="flex items-center justify-between text-xs" style={{ color: colors.text.muted }}>
        <div className="flex items-center gap-2">
          <div
            className="h-1.5 w-1.5 rounded-full"
            style={{ backgroundColor: getSeverityBorderColor(level) }}
          />
          <TimeAgo timestamp={ts} className="text-text-secondary" />
        </div>
        <span className="font-mono" style={{ color: colors.text.muted }}>{alert.strategy_id || '-'}</span>
      </div>
      <button
        type="button"
        onClick={() => onRowClick(alert)}
        className="flex items-center justify-between rounded-lg border px-3 py-2 text-xs transition-colors hover:bg-bg-hover"
        style={{ borderColor: colors.border.DEFAULT, color: colors.text.secondary }}
      >
        <span>View details</span>
        <span style={{ color: colors.text.muted }}>▶</span>
      </button>
    </div>
  );
};

/**
 * Inline alert details (non-modal expansion)
 */
function InlineAlertDetails({ alert }: { alert: Alert }) {
  const [showFullJSON, setShowFullJSON] = useState(false);

  const context = alert.context as any;
  const details = alert.details as any;

  const alertJson = useMemo(() => {
    const seen = new WeakSet<object>();
    const MAX_LENGTH = 50000;

    const helper = (val: any): any => {
      if (val === null || val === undefined) return val;
      if (typeof val !== 'object') return val;
      if (seen.has(val)) return '[Circular]';
      seen.add(val);
      if (Array.isArray(val)) return val.map((v) => helper(v));
      const out: Record<string, any> = {};
      for (const key of Object.keys(val).sort()) {
        out[key] = helper(val[key]);
      }
      return out;
    };

    try {
      const result = JSON.stringify(helper(alert), null, 2);
      return result.length > MAX_LENGTH
        ? result.substring(0, MAX_LENGTH) + '\n\n... [Truncated]'
        : result;
    } catch {
      return JSON.stringify(alert, null, 2);
    }
  }, [alert]);

  return (
    <div className="space-y-3">
      {/* Context Section */}
      {context && (
        <div>
          <div className="text-sm font-semibold mb-2" style={{ color: colors.text.secondary }}>Context</div>
          <div className="font-mono text-xs p-3 rounded-lg space-y-2 break-words border" style={{ backgroundColor: colors.bg.base, borderColor: colors.border.DEFAULT, color: colors.text.primary }}>
            {context.error && (
              <div className="break-words">
                <span className="text-red-400">Error:</span> {String(context.error)}
              </div>
            )}
            {context.error_type && (
              <div><span style={{ color: colors.text.muted }}>Type:</span> {String(context.error_type)}</div>
            )}
            {typeof context.edge_bps === 'number' && Number.isFinite(context.edge_bps) && (
              <div><span style={{ color: colors.text.muted }}>Edge:</span> {context.edge_bps.toFixed(2)} bps</div>
            )}
            {context.order_id && (
              <div><span style={{ color: colors.text.muted }}>Order:</span> {String(context.order_id)}</div>
            )}
          </div>
        </div>
      )}

      {/* Full JSON Section */}
      <div>
        <button
          onClick={() => setShowFullJSON(!showFullJSON)}
          className="text-sm font-semibold hover:text-text-primary cursor-pointer mb-2 flex items-center gap-2"
          style={{ color: colors.text.secondary }}
        >
          <span>{showFullJSON ? '▼' : '▶'}</span>
          Full JSON
        </button>
        {showFullJSON && (
          <div className="font-mono text-xs p-3 rounded-lg overflow-x-auto max-h-96 overflow-y-auto border" style={{ backgroundColor: colors.bg.base, borderColor: colors.border.DEFAULT, color: colors.text.primary }}>
            <pre className="whitespace-pre-wrap break-words">{alertJson}</pre>
          </div>
        )}
      </div>
    </div>
  );
}
