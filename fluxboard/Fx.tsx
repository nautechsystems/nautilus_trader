// FX rates page with auto-refresh and backoff

import { useEffect, useCallback, useState } from 'react';
import { api } from './api';
import { useFxStore, selectFxLastFetch } from './stores';
import FxTable from './FxTable';
import StrategyFxTable from './StrategyFxTable';
import type { StrategyFxConfig } from './types';
import { PanelHeader } from './components/shared/PanelHeader';
import { PanelBody } from './components/shared/PanelBody';
import { Badge } from './components/ui/badge';
import { cn } from './lib/utils';
import { STALE_THRESHOLDS, colors } from './lib/tokens';
import { PageShell } from './components/layout/PageShell';

export default function Fx({
  dense = false,
  className = '',
  onRemove,
  showHeader = true,
}: {
  dense?: boolean;
  className?: string;
  onRemove?: () => void;
  showHeader?: boolean;
} = {}) {
  const {
    loading,
    error,
    data,
    auto,
    backoffMs,
    setLoading,
    setError,
    setData,
    setAuto,
    resetBackoff,
    increaseBackoff
  } = useFxStore();
  const lastUpdate = useFxStore(selectFxLastFetch);

  // State for strategy FX configuration
  const [strategyConfigs, setStrategyConfigs] = useState<StrategyFxConfig[]>([]);
  const [strategyLoading, setStrategyLoading] = useState(true);

  const fetchOnce = useCallback(async () => {
    setLoading(true);
    try {
      const result = await api.getFxDashboard();
      setData(result);
      resetBackoff(); // Reset to intervalMs on success
    } catch (e) {
      const msg = e instanceof Error ? e.message : 'Failed to fetch FX dashboard';
      setError(msg);
      increaseBackoff(); // Increase backoff on error
    } finally {
      setLoading(false);
    }
  }, [setLoading, setError, setData, resetBackoff, increaseBackoff]);

  const fetchStrategyConfigs = useCallback(async () => {
    setStrategyLoading(true);
    try {
      const configs = await api.getStrategyFxConfig();
      setStrategyConfigs(configs);
    } catch (e) {
      if (import.meta.env?.DEV) {
        console.error('[fx] Failed to fetch strategy configs:', e);
      }
      // Don't show error to user, just log it
    } finally {
      setStrategyLoading(false);
    }
  }, []);

  // Initial fetch
  useEffect(() => {
    fetchOnce();
    fetchStrategyConfigs();
  }, [fetchOnce, fetchStrategyConfigs]);

  // Auto-refresh with backoff
  useEffect(() => {
    if (!auto) return;

    const interval = setInterval(() => {
      fetchOnce();
    }, backoffMs);

    return () => clearInterval(interval);
  }, [auto, backoffMs, fetchOnce]);

  const handleRefresh = () => {
    fetchOnce();
  };

  const handleToggleAuto = () => {
    setAuto(!auto);
  };

  const serviceOk = data?.service.ok || false;
  const uptimeSec = data?.service.uptime_s || 0;
  const version = data?.service.version || '—';
  const pairs = data?.pairs || [];

  const fmtUptime = (sec: number): string => {
    if (sec < 60) return `${Math.floor(sec)}s`;
    const min = Math.floor(sec / 60);
    if (min < 60) return `${min}m`;
    const hr = Math.floor(min / 60);
    const remainMin = min % 60;
    return `${hr}h ${remainMin}m`;
  };

  const headerActions = (
    <div className="flex items-center gap-3">
      <Badge variant={serviceOk ? 'success' : 'danger'} size="xs">
        {serviceOk ? 'Service OK' : 'Service Down'}
      </Badge>
      <span className="text-xs text-zinc-500">
        Uptime: {fmtUptime(uptimeSec)}
      </span>
      <span className="text-xs text-zinc-500">
        Version: {version}
      </span>
      {loading && (
        <span className="text-xs text-amber-500">
          Loading…
        </span>
      )}
      <label className="flex items-center gap-2 text-xs text-zinc-400 cursor-pointer select-none hover:text-zinc-300 transition-colors">
        <input
          type="checkbox"
          checked={auto}
          onChange={handleToggleAuto}
          className="h-3.5 w-3.5 rounded border-zinc-700 bg-zinc-900 text-emerald-500 focus:ring-emerald-500/20"
        />
        <span className="font-medium">
          Auto ({(backoffMs / 1000).toFixed(1)}s)
        </span>
      </label>
    </div>
  );

  const content = (
    <div className={cn("flex flex-col h-full overflow-hidden", className)} style={{ color: colors.text.secondary }}>
      {showHeader && (
        <PanelHeader
          title="FX Dashboard"
          onRefresh={handleRefresh}
          refreshing={loading}
          lastUpdate={lastUpdate}
          staleThresholdMs={STALE_THRESHOLDS.FAST}
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

      <PanelBody>
        <div className={cn("flex flex-col gap-4", dense ? "p-3" : "p-6")}>
          {error && (
            <div className="rounded-xl border border-red-900/50 bg-red-950/30 p-4 text-sm text-red-400">
              <span className="font-semibold">Error:</span> {error}
            </div>
          )}

          <div className="rounded-xl border border-neutral-800 bg-neutral-900/30 overflow-hidden">
            <FxTable pairs={pairs} />
          </div>

          {data && (
            <div className="rounded-xl border border-neutral-800 bg-neutral-900/30 p-4 text-xs text-neutral-500">
              <div className="flex items-center gap-6">
                <div>
                  <span className="font-semibold text-neutral-200">Bybit:</span>{' '}
                  {data.bybit?.connected ? (
                    <span className="text-success-light">Connected</span>
                  ) : (
                    <span className="text-danger-light">Disconnected</span>
                  )}
                  {data.bybit?.reconnects !== undefined && (
                    <span className="ml-2 text-neutral-500">
                      ({data.bybit.reconnects} reconnect{data.bybit.reconnects !== 1 ? 's' : ''})
                    </span>
                  )}
                </div>

                <div>
                  <span className="font-semibold text-neutral-200">Curve:</span>{' '}
                  {data.curve?.polls || 0} pool{data.curve?.polls !== 1 ? 's' : ''}
                </div>
              </div>
            </div>
          )}

          <div className="flex flex-col gap-3 rounded-xl border border-neutral-800 bg-neutral-900/30 p-4">
            <h2 className="text-sm font-semibold text-neutral-200 m-0">
              Strategy FX Configuration
            </h2>
            {strategyLoading ? (
              <div className="p-4 text-center text-xs text-neutral-500">
                Loading strategy configurations...
              </div>
            ) : (
              <StrategyFxTable strategies={strategyConfigs} />
            )}
          </div>
        </div>
      </PanelBody>
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
