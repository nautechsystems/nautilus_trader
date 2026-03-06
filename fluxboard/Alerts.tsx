/**
 * Alerts Panel - Alerts view with filtering, auto-dismiss, and live updates
 *
 * Displays system alerts with level filtering (CRITICAL, WARNING, INFO).
 * Updates via WebSocket with fallback to REST API polling.
 * Auto-dismisses INFO (10s) and WARNING (30s) alerts.
 */

import { useState, useCallback, useMemo, useRef } from 'react';
import { api, normalizeAlertsSnapshotCandidate } from './api';
import { useAlertsStore } from './stores';
import { INTERVALS } from './constants';
import { usePolling, useWebSocket } from './hooks/index';
import type { Alert, AlertLevel } from './types';
import { AlertsTable, AlertDetails } from './components/domain/alerts';
import { PanelHeader } from './components/shared/PanelHeader';
import { PanelBody } from './components/shared/PanelBody';
import { PageShell } from './components/layout/PageShell';
import { Button } from './components/ui/button/Button';
import { Dialog } from './components/ui/dialog/Dialog';
import { Switch } from './components/ui/switch';
import { Select } from './components/ui/select';
import { colors, STALE_THRESHOLDS, spacing, typography } from './lib/tokens';
import { useMobileLayout } from './hooks/useMobileLayout';

export default function Alerts({
  dense = false,
  onRemove,
  showHeader = true,
}: {
  dense?: boolean;
  onRemove?: () => void;
  showHeader?: boolean;
} = {}) {
  // Use selective subscriptions to prevent unnecessary re-renders
  const rows = useAlertsStore((state) => state.rows);
  const loading = useAlertsStore((state) => state.loading);
  const auto = useAlertsStore((state) => state.auto);
  const dismissedIds = useAlertsStore((state) => state.dismissedIds);
  const setRows = useAlertsStore((state) => state.setRows);
  const setLoading = useAlertsStore((state) => state.setLoading);
  const setAuto = useAlertsStore((state) => state.setAuto);
  const dismissAlert = useAlertsStore((state) => state.dismissAlert);
  const clearAlerts = useAlertsStore((state) => state.clearAlerts);
  const [levelFilter, setLevelFilter] = useState<AlertLevel | 'ALL'>('ALL');
  const [expandedAlertId, setExpandedAlertId] = useState<string | null>(null);
  const [showClearConfirm, setShowClearConfirm] = useState(false);
  const [lastUpdate, setLastUpdate] = useState<number>(Date.now());
  const [refreshing, setRefreshing] = useState(false);
  const { isMobile } = useMobileLayout();
  const [selectedAlert, setSelectedAlert] = useState<Alert | null>(null);

  // Track last WebSocket data to prevent redundant updates
  const lastWebSocketDataRef = useRef<string>('');
  const lastAlertsSummaryRef = useRef<string>('');
  const pendingAlertsSummaryRef = useRef<string>('');
  const hasLoadedRef = useRef(false);

  const refreshAlertsFromApi = useCallback(async (options?: { showLoading?: boolean; summaryKey?: string }) => {
    const shouldShowLoading = Boolean(options?.showLoading);
    const summaryKey = options?.summaryKey ?? '';
    if (shouldShowLoading) {
      setLoading(true);
    }
    if (summaryKey) {
      pendingAlertsSummaryRef.current = summaryKey;
    }
    try {
      const data = await api.getAlerts();
      setRows(data);
      setLastUpdate(Date.now());
      hasLoadedRef.current = true;
      if (summaryKey) {
        lastAlertsSummaryRef.current = summaryKey;
      }
    } catch (e) {
      if (import.meta.env?.DEV) {
        console.error('[alerts] Failed to load:', e);
      }
    } finally {
      if (summaryKey && pendingAlertsSummaryRef.current === summaryKey) {
        pendingAlertsSummaryRef.current = '';
      }
      if (shouldShowLoading) {
        setLoading(false);
      }
    }
  }, [setRows, setLoading]);

  // Load alerts from API (only show loading on first load)
  const loadAlerts = useCallback(async () => {
    const isFirstLoad = !hasLoadedRef.current;
    await refreshAlertsFromApi({ showLoading: isFirstLoad });
  }, [refreshAlertsFromApi]);

  // Auto-refresh polling with usePolling hook
  usePolling(loadAlerts, INTERVALS.ALERTS_POLL, auto);

  // Subscribe to live alert updates via WebSocket using useWebSocket hook
  useWebSocket<{ alerts?: unknown[] | { count?: number; latest_ts_ms?: number | null }; rows?: unknown[] }>(
    'market_update',
    useCallback(
      (payload) => {
        const alertsSummary = payload && typeof payload === 'object' ? (payload as any).alerts : undefined;
        if (
          alertsSummary
          && typeof alertsSummary === 'object'
          && !Array.isArray(alertsSummary)
        ) {
          const summaryKey = `summary:${String((alertsSummary as any).count ?? '')}:${String((alertsSummary as any).latest_ts_ms ?? '')}`;
          if (
            lastAlertsSummaryRef.current !== summaryKey
            && pendingAlertsSummaryRef.current !== summaryKey
          ) {
            void refreshAlertsFromApi({ summaryKey });
          }
          return;
        }

        const hasSnapshotPayload = Boolean(
          payload
          && typeof payload === 'object'
          && (Array.isArray((payload as any).alerts) || Array.isArray((payload as any).rows)),
        );
        if (!hasSnapshotPayload) return;

        // Legacy Socket.IO snapshots sometimes shipped `alerts: ['id-a', 'id-b']` with no row data.
        // Treat those as no-ops so we don't mistakenly clear the UI.
        const rawAlertsCandidate = (payload as any).alerts ?? (payload as any).rows;
        if (
          Array.isArray(rawAlertsCandidate)
          && rawAlertsCandidate.length > 0
          && rawAlertsCandidate.every(
            (item) => typeof item === 'string' && !String(item).trim().startsWith('{'),
          )
        ) {
          return;
        }

        const parsedAlerts = normalizeAlertsSnapshotCandidate(payload);
        if (parsedAlerts.length === 0) {
          if (lastWebSocketDataRef.current === '__empty__') return;
          lastWebSocketDataRef.current = '__empty__';
          setRows([]);
          setLastUpdate(Date.now());
          return;
        }

        try {
          // Deduplicate: hash full alert content (not just IDs) to detect any changes
          const dataHash = JSON.stringify(
            parsedAlerts.map(a => ({ id: a.id, ts: a.ts || a.timestamp, title: a.title, severity: a.severity || a.level }))
              .sort((a, b) => a.id.localeCompare(b.id))
          );
          if (lastWebSocketDataRef.current === dataHash) {
            return; // Same data, skip update
          }
          lastWebSocketDataRef.current = dataHash;

          // Set full alert list (backend sends complete snapshot)
          setRows(parsedAlerts);
          setLastUpdate(Date.now());
        } catch (e) {
          if (import.meta.env?.DEV) {
            console.error('[alerts] Failed to parse WebSocket alerts:', e);
          }
        }
      },
      [setRows, lastWebSocketDataRef, refreshAlertsFromApi]
    )
  );

  // Manual refresh handler
  const handleRefresh = useCallback(async () => {
    setRefreshing(true);
    try {
      const data = await api.getAlerts();
      setRows(data);
      setLastUpdate(Date.now());
    } catch (e) {
      if (import.meta.env?.DEV) {
        console.error('[alerts] Refresh failed:', e);
      }
    } finally {
      setRefreshing(false);
    }
  }, [setRows]);

  const handleClearAll = useCallback(async () => {
    try {
      await api.clearAlerts();
      clearAlerts();
      setShowClearConfirm(false);
    } catch (e) {
      if (import.meta.env?.DEV) {
        console.error('[alerts] Failed to clear alerts:', e);
      }
    }
  }, [clearAlerts]);

  // Filter alerts by level (memoized to prevent unnecessary re-renders)
  const filteredRows = useMemo(
    () => rows.filter(
      (a) => !dismissedIds.has(a.id) && (levelFilter === 'ALL' || (a.severity || a.level) === levelFilter)
    ),
    [rows, dismissedIds, levelFilter]
  );

  // Extract header actions (following Balances.tsx pattern)
  const headerActions = (
    <div className="flex items-center gap-3">
      {/* Level Filter */}
      <Select
        size="xs"
        value={levelFilter}
        onChange={(value) => setLevelFilter(value as AlertLevel | 'ALL')}
        options={[
          { label: 'All Levels', value: 'ALL' },
          { label: 'Critical', value: 'CRITICAL' },
          { label: 'Warning', value: 'WARNING' },
          { label: 'Info', value: 'INFO' },
        ]}
      />

      {/* Auto-refresh Toggle */}
      <Switch
        size="sm"
        checked={auto}
        onCheckedChange={setAuto}
        label="Auto"
      />

      {/* Alert Count */}
      <span className="text-xs" style={{ color: colors.text.muted }}>
        {filteredRows.length} alert{filteredRows.length !== 1 ? 's' : ''}
      </span>

      {/* Clear All Button */}
      {filteredRows.length > 0 && (
        <Button variant="ghost" size="xs" onClick={() => setShowClearConfirm(true)}>
          Clear All
        </Button>
      )}
    </div>
  );

  const handleRowClick = useCallback((alert: Alert) => {
    if (isMobile) {
      setSelectedAlert(alert);
    } else {
      setExpandedAlertId((prev) => (prev === alert.id ? null : alert.id));
    }
  }, [isMobile]);

  const content = (
    <div className="flex flex-col h-full overflow-hidden">
      {showHeader && (
        <PanelHeader
          title="Alerts"
          onRefresh={handleRefresh}
          refreshing={refreshing}
          lastUpdate={lastUpdate}
          staleThresholdMs={STALE_THRESHOLDS.NORMAL}
          onRemove={onRemove}
          actions={headerActions}
        />
      )}

      {/* When embedded in dashboard (showHeader=false), render actions as toolbar */}
      {!showHeader && (
        <div className="flex items-center justify-end border-b px-4 py-2 gap-2" style={{ borderColor: colors.border.DEFAULT, backgroundColor: colors.bg.surface }}>
          {headerActions}
        </div>
      )}

      {/* Table with inline expansion */}
      <PanelBody>
        <AlertsTable
          alerts={rows}
          loading={loading}
          dismissedIds={dismissedIds}
          levelFilter={levelFilter}
          onDismiss={dismissAlert}
          onRowClick={handleRowClick}
          expandedAlertId={expandedAlertId}
          dense={dense}
        />
      </PanelBody>

      {/* Clear All Confirmation Dialog */}
      <Dialog isOpen={showClearConfirm} onClose={() => setShowClearConfirm(false)} title="Clear All Alerts" size="sm">
        <p className="text-sm text-text-muted mb-4">
          Are you sure you want to clear all alerts? This action cannot be undone.
        </p>
        <div className="flex items-center justify-end gap-2">
          <Button variant="ghost" size="sm" onClick={() => setShowClearConfirm(false)}>
            Cancel
          </Button>
          <Button variant="destructive" size="sm" onClick={handleClearAll}>
            Clear All
          </Button>
        </div>
      </Dialog>

      <AlertDetails
        alert={selectedAlert}
        isOpen={Boolean(selectedAlert)}
        onClose={() => setSelectedAlert(null)}
      />
    </div>
  );

  if (showHeader) {
    return (
      <PageShell>
        {content}
      </PageShell>
    );
  }

  return content;
}
