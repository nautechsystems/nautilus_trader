/**
 * Alerts Panel - Alerts view with filtering, auto-dismiss, and live updates
 *
 * Displays system alerts with level filtering (CRITICAL, ERROR, WARNING, INFO).
 * Updates via WebSocket with fallback to REST API polling.
 * Auto-dismisses INFO (10s) and WARNING (30s) alerts.
 */

import { useState, useCallback, useEffect, useMemo, useRef } from 'react';
import { api, normalizeAlertsSnapshotCandidate } from './api';
import { useAlertsStore } from './stores';
import { INTERVALS } from './constants';
import { usePolling, useWebSocket } from './hooks/index';
import type { Alert, AlertLevel } from './types';
import { AlertsTable, AlertDetails } from './components/domain/alerts';
import { PanelHeader } from './components/shared/PanelHeader';
import { PanelBody } from './components/shared/PanelBody';
import { StatusPill } from './components/shared/StatusPill';
import { PageShell } from './components/layout/PageShell';
import { Button } from './components/ui/button/Button';
import { Dialog } from './components/ui/dialog/Dialog';
import { Switch } from './components/ui/switch';
import { Select } from './components/ui/select';
import { isRealtimeStandardEnabled } from './config/featureFlags';
import { RealtimeSurfaceState } from './lib/realtime/types';
import { colors, STALE_THRESHOLDS, spacing, typography } from './lib/tokens';
import { useMobileLayout } from './hooks/useMobileLayout';

function resolveAlertsSurfaceStatus(state: RealtimeSurfaceState): {
  label: string;
  status: 'ok' | 'warning' | 'critical' | 'muted';
} {
  switch (state) {
    case RealtimeSurfaceState.LIVE:
      return { label: 'LIVE', status: 'ok' };
    case RealtimeSurfaceState.LAGGING:
      return { label: 'LAGGING', status: 'warning' };
    case RealtimeSurfaceState.STALE:
      return { label: 'STALE', status: 'critical' };
    case RealtimeSurfaceState.RECOVERING:
      return { label: 'RECOVERING', status: 'warning' };
    case RealtimeSurfaceState.SYNCING:
      return { label: 'SYNCING', status: 'muted' };
    case RealtimeSurfaceState.MANUAL_REFRESH_REQUIRED:
      return { label: 'REFRESH', status: 'critical' };
    default:
      return { label: 'SYNCING', status: 'muted' };
  }
}

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
  const alertsRealtimeStandardEnabled = useMemo(() => isRealtimeStandardEnabled('alerts'), []);
  const [surfaceState, setSurfaceState] = useState<RealtimeSurfaceState>(() => (
    alertsRealtimeStandardEnabled ? RealtimeSurfaceState.SYNCING : RealtimeSurfaceState.LIVE
  ));
  const { isMobile } = useMobileLayout();
  const [selectedAlert, setSelectedAlert] = useState<Alert | null>(null);

  // Track last WebSocket data to prevent redundant updates
  const lastWebSocketDataRef = useRef<string>('');
  const lastAlertsSummaryRef = useRef<string>('');
  const pendingAlertsSummaryRef = useRef<string>('');
  const summaryRefreshRequestIdRef = useRef(0);
  const hasLoadedRef = useRef(false);
  const lastUpdateRef = useRef(lastUpdate);

  const syncSurfaceState = useCallback(() => {
    if (!alertsRealtimeStandardEnabled) {
      return;
    }
    setSurfaceState((current) => {
      if (current === RealtimeSurfaceState.RECOVERING) {
        return current;
      }
      if (loading && !hasLoadedRef.current) {
        return RealtimeSurfaceState.SYNCING;
      }
      if (!hasLoadedRef.current) {
        return RealtimeSurfaceState.SYNCING;
      }

      const ageMs = Date.now() - lastUpdateRef.current;
      if (ageMs > STALE_THRESHOLDS.NORMAL * 2) {
        return RealtimeSurfaceState.STALE;
      }
      if (ageMs > STALE_THRESHOLDS.NORMAL) {
        return RealtimeSurfaceState.LAGGING;
      }
      return RealtimeSurfaceState.LIVE;
    });
  }, [alertsRealtimeStandardEnabled, loading]);

  const refreshAlertsFromApi = useCallback(async (options?: { showLoading?: boolean; summaryKey?: string }) => {
    const shouldShowLoading = Boolean(options?.showLoading);
    const summaryKey = options?.summaryKey ?? '';
    const summaryRequestId = summaryKey ? (summaryRefreshRequestIdRef.current + 1) : 0;
    if (alertsRealtimeStandardEnabled) {
      setSurfaceState(summaryKey ? RealtimeSurfaceState.RECOVERING : RealtimeSurfaceState.SYNCING);
    }
    if (shouldShowLoading) {
      setLoading(true);
    }
    if (summaryKey) {
      summaryRefreshRequestIdRef.current = summaryRequestId;
      pendingAlertsSummaryRef.current = summaryKey;
    }
    try {
      const data = await api.getAlerts();
      if (summaryKey && summaryRequestId !== summaryRefreshRequestIdRef.current) {
        return;
      }
      setRows(data);
      const receivedAt = Date.now();
      setLastUpdate(receivedAt);
      lastUpdateRef.current = receivedAt;
      hasLoadedRef.current = true;
      if (alertsRealtimeStandardEnabled) {
        setSurfaceState(RealtimeSurfaceState.LIVE);
      }
      if (summaryKey) {
        lastAlertsSummaryRef.current = summaryKey;
      }
    } catch (e) {
      if (alertsRealtimeStandardEnabled) {
        setSurfaceState(RealtimeSurfaceState.STALE);
      }
      if (import.meta.env?.DEV) {
        console.error('[alerts] Failed to load:', e);
      }
    } finally {
      if (
        summaryKey
        && summaryRequestId === summaryRefreshRequestIdRef.current
        && pendingAlertsSummaryRef.current === summaryKey
      ) {
        pendingAlertsSummaryRef.current = '';
      }
      if (shouldShowLoading) {
        setLoading(false);
      }
    }
  }, [alertsRealtimeStandardEnabled, setRows, setLoading]);

  // Load alerts from API (only show loading on first load)
  const loadAlerts = useCallback(async () => {
    const isFirstLoad = !hasLoadedRef.current;
    await refreshAlertsFromApi({ showLoading: isFirstLoad });
  }, [refreshAlertsFromApi]);

  useEffect(() => {
    lastUpdateRef.current = lastUpdate;
  }, [lastUpdate]);

  useEffect(() => {
    if (!alertsRealtimeStandardEnabled) {
      return;
    }
    void loadAlerts();
  }, [alertsRealtimeStandardEnabled, loadAlerts]);

  useEffect(() => {
    if (!alertsRealtimeStandardEnabled) {
      return undefined;
    }
    syncSurfaceState();
    const intervalId = window.setInterval(() => {
      syncSurfaceState();
    }, 1_000);
    return () => {
      window.clearInterval(intervalId);
    };
  }, [alertsRealtimeStandardEnabled, syncSurfaceState]);

  const pollingEnabled = alertsRealtimeStandardEnabled
    ? auto && (
      surfaceState === RealtimeSurfaceState.LAGGING
      || surfaceState === RealtimeSurfaceState.STALE
    )
    : auto;

  // Auto-refresh polling with usePolling hook
  usePolling(loadAlerts, INTERVALS.ALERTS_POLL, pollingEnabled);

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
            void refreshAlertsFromApi({ showLoading: !hasLoadedRef.current, summaryKey });
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
          const receivedAt = Date.now();
          setLastUpdate(receivedAt);
          lastUpdateRef.current = receivedAt;
          if (alertsRealtimeStandardEnabled) {
            setSurfaceState(RealtimeSurfaceState.LIVE);
          }
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
          const receivedAt = Date.now();
          setLastUpdate(receivedAt);
          lastUpdateRef.current = receivedAt;
          if (alertsRealtimeStandardEnabled) {
            setSurfaceState(RealtimeSurfaceState.LIVE);
          }
        } catch (e) {
          if (import.meta.env?.DEV) {
            console.error('[alerts] Failed to parse WebSocket alerts:', e);
          }
        }
      },
      [alertsRealtimeStandardEnabled, setRows, refreshAlertsFromApi]
    ),
    { surface: 'alerts' }
  );

  // Manual refresh handler
  const handleRefresh = useCallback(async () => {
    setRefreshing(true);
    if (alertsRealtimeStandardEnabled) {
      setSurfaceState(RealtimeSurfaceState.RECOVERING);
    }
    try {
      const data = await api.getAlerts();
      setRows(data);
      const refreshedAt = Date.now();
      setLastUpdate(refreshedAt);
      lastUpdateRef.current = refreshedAt;
      if (alertsRealtimeStandardEnabled) {
        setSurfaceState(RealtimeSurfaceState.LIVE);
      }
    } catch (e) {
      if (alertsRealtimeStandardEnabled) {
        setSurfaceState(RealtimeSurfaceState.STALE);
      }
      if (import.meta.env?.DEV) {
        console.error('[alerts] Refresh failed:', e);
      }
    } finally {
      setRefreshing(false);
    }
  }, [alertsRealtimeStandardEnabled, setRows]);

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
          { label: 'Error', value: 'ERROR' },
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

  const surfaceStatus = alertsRealtimeStandardEnabled
    ? resolveAlertsSurfaceStatus(surfaceState)
    : null;
  const titleActions = surfaceStatus ? (
    <StatusPill
      status={surfaceStatus.status}
      label={surfaceStatus.label}
      layout="inline"
      size="xs"
      tone="subtle"
    />
  ) : undefined;

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
          titleActions={titleActions}
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
