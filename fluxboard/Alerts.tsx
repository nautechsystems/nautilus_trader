/**
 * Alerts Panel - Alerts view with filtering, auto-dismiss, and live updates
 *
 * Displays system alerts with level filtering (CRITICAL, WARNING, INFO).
 * Updates via WebSocket with fallback to REST API polling.
 * Auto-dismisses INFO (10s) and WARNING (30s) alerts.
 */

import { useState, useCallback, useMemo, useRef } from 'react';
import { api } from './api';
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
  const hasLoadedRef = useRef(false);

  // Load alerts from API (only show loading on first load)
  const loadAlerts = useCallback(async () => {
    const isFirstLoad = !hasLoadedRef.current;
    if (isFirstLoad) {
      setLoading(true);
    }
    try {
      const data = await api.getAlerts();
      setRows(data);
      setLastUpdate(Date.now());
      hasLoadedRef.current = true;
    } catch (e) {
      if (import.meta.env?.DEV) {
        console.error('[alerts] Failed to load:', e);
      }
      setRows([]);
    } finally {
      if (isFirstLoad) {
        setLoading(false);
      }
    }
  }, [setRows, setLoading]);

  // Auto-refresh polling with usePolling hook
  usePolling(loadAlerts, INTERVALS.ALERTS_POLL, auto);

  // Subscribe to live alert updates via WebSocket using useWebSocket hook
  useWebSocket<{ alerts?: Alert[] | string[] }>(
    'market_update',
    useCallback(
      (payload) => {
        if (!payload || !Array.isArray((payload as any).alerts)) return;
        const arr = (payload as any).alerts as Array<Alert | string>;

        // Ignore legacy id-only snapshots (string[] of IDs) to avoid clearing UI
        const allStrings = arr.length > 0 && arr.every((x) => typeof x === 'string');
        if (allStrings) {
          try {
            const first = JSON.parse(arr[0] as string);
            if (!first || typeof first !== 'object') return; // not a JSON object payload
          } catch {
            return; // not JSON strings; ignore
          }
        }

        try {
          // Parse alerts (they may come as JSON strings from Redis)
          const parsedAlerts: Alert[] = arr
            .map((item) => (typeof item === 'string' ? (JSON.parse(item) as Alert) : item))
            .filter((a): a is Alert => !!a && typeof a === 'object' && 'id' in a);

          if (!Array.isArray(parsedAlerts) || parsedAlerts.length === 0) return;

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
      [setRows, lastWebSocketDataRef]
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
          <Button variant="danger" size="sm" onClick={handleClearAll}>
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
