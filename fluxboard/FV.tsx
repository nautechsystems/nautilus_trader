import { useCallback, useEffect, useMemo, useRef, useState } from 'react';

import { api } from './api';
import {
  selectFvLastFetch,
  useFvStore,
} from './stores';
import type { FvSnapshot } from './types';
import { useWebSocket } from './hooks/useWebSocket';
import { PageShell } from './components/layout/PageShell';
import { PanelHeader } from './components/shared/PanelHeader';
import { PanelBody } from './components/shared/PanelBody';
import { Badge } from './components/ui/badge';
import { Select, Switch } from './components/ui';
import { STALE_THRESHOLDS } from './lib/tokens';
import { cn } from './lib/utils';
import {
  DEFAULT_FV_PROFILE,
  mergeSnapshotWithStickyWhatMoved,
  normalizeProfile,
} from './lib/fvSnapshot';
import { FVTable } from './components/domain/fv/FVTable';
import { FVBreakdown } from './components/domain/fv/FVBreakdown';
import { FVFormulaInspector } from './components/domain/fv/FVFormulaInspector';
import { FVSpecSummary } from './components/domain/fv/FVSpecSummary';
import { FVWhatMoved } from './components/domain/fv/FVWhatMoved';

const WS_FLUSH_INTERVAL_MS = 250;

export default function FV({
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
    error,
    profile,
    profiles,
    symbols,
    symbol,
    latest,
    auto,
    backoffMs,
    setError,
    setProfile,
    setProfiles,
    setSymbols,
    setSymbol,
    setLatest,
    setAuto,
    resetBackoff,
    increaseBackoff,
  } = useFvStore();
  const lastUpdate = useFvStore(selectFvLastFetch);
  const [selectedTermId, setSelectedTermId] = useState<number | undefined>(undefined);
  const [manualRefreshing, setManualRefreshing] = useState(false);
  const [config, setConfig] = useState<Record<string, unknown> | undefined>(undefined);
  const [configLoading, setConfigLoading] = useState(false);
  const [configError, setConfigError] = useState<string | undefined>(undefined);
  const pendingWsSnapshotRef = useRef<FvSnapshot | undefined>(undefined);
  const wsFlushTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const previousSnapshotRef = useRef<FvSnapshot | undefined>(undefined);

  const applyLatestSnapshot = useCallback((snapshot: FvSnapshot) => {
    const previous = useFvStore.getState().latest;
    setLatest(mergeSnapshotWithStickyWhatMoved(previous, snapshot));
  }, [setLatest]);

  const ensureProfileKnown = useCallback((candidate?: string) => {
    const nextProfile = normalizeProfile(candidate);
    const currentProfiles = useFvStore.getState().profiles || [];
    if (!currentProfiles.includes(nextProfile)) {
      setProfiles([...currentProfiles, nextProfile]);
    }
  }, [setProfiles]);

  const flushPendingWsSnapshot = useCallback(() => {
    const snapshot = pendingWsSnapshotRef.current;
    pendingWsSnapshotRef.current = undefined;
    if (!snapshot) return;
    applyLatestSnapshot(snapshot);
    ensureProfileKnown(snapshot.fv_profile);
    resetBackoff();
  }, [applyLatestSnapshot, ensureProfileKnown, resetBackoff]);

  const fetchSymbols = useCallback(async (targetProfile: string) => {
    try {
      const nextSymbols = await api.getFvSymbols(targetProfile);
      setSymbols(nextSymbols);
      if (!nextSymbols.length) {
        setError(`No FV symbols published yet for profile ${targetProfile}.`);
      }
    } catch (e) {
      const message = e instanceof Error ? e.message : 'Failed to fetch FV symbols';
      setError(message);
    }
  }, [setError, setSymbols]);

  const fetchLatest = useCallback(async (targetSymbol?: string, targetProfile?: string) => {
    if (!targetSymbol) return;
    const profileToQuery = normalizeProfile(targetProfile || profile);
    try {
      const snapshot = await api.getFvLatest(targetSymbol, { fvProfile: profileToQuery });
      if (snapshot) {
        applyLatestSnapshot(snapshot);
        ensureProfileKnown(snapshot.fv_profile || profileToQuery);
      }
      setError(undefined);
      resetBackoff();
    } catch (e) {
      const message = e instanceof Error ? e.message : 'Failed to fetch FV snapshot';
      setError(message);
      increaseBackoff();
    }
  }, [applyLatestSnapshot, ensureProfileKnown, increaseBackoff, profile, resetBackoff, setError]);

  const fetchConfig = useCallback(async (targetSymbol?: string, targetProfile?: string) => {
    if (!targetSymbol) {
      setConfig(undefined);
      setConfigError(undefined);
      return;
    }
    const profileToQuery = normalizeProfile(targetProfile || profile);
    setConfigLoading(true);
    try {
      const nextConfig = await api.getFvConfig(targetSymbol, { fvProfile: profileToQuery });
      setConfig(nextConfig);
      setConfigError(undefined);
    } catch (e) {
      const message = e instanceof Error ? e.message : 'Failed to fetch FV config';
      setConfigError(message);
    } finally {
      setConfigLoading(false);
    }
  }, [profile]);

  useEffect(() => {
    fetchSymbols(profile);
  }, [fetchSymbols, profile]);

  useEffect(() => {
    if (!symbol && symbols.length > 0) {
      setSymbol(symbols[0]);
    }
  }, [symbol, symbols, setSymbol]);

  useEffect(() => {
    if (!symbol) return;
    fetchLatest(symbol, profile);
  }, [fetchLatest, profile, symbol]);

  useEffect(() => {
    if (!symbol) return;
    fetchConfig(symbol, profile);
  }, [fetchConfig, profile, symbol]);

  useEffect(() => {
    if (!auto || !symbol) return;
    const timer = setInterval(() => fetchLatest(symbol, profile), backoffMs);
    return () => clearInterval(timer);
  }, [auto, backoffMs, fetchLatest, profile, symbol]);

  useEffect(() => () => {
    if (wsFlushTimerRef.current) {
      clearTimeout(wsFlushTimerRef.current);
      wsFlushTimerRef.current = null;
    }
  }, []);

  useEffect(() => {
    previousSnapshotRef.current = latest;
  }, [latest]);

  useWebSocket<FvSnapshot>('fv_update', (payload) => {
    if (!payload || !payload.symbol) return;
    if (symbol && payload.symbol !== symbol) return;
    const payloadProfile = normalizeProfile(payload.fv_profile);
    if (payloadProfile !== profile) return;
    pendingWsSnapshotRef.current = payload;
    if (!wsFlushTimerRef.current) {
      wsFlushTimerRef.current = setTimeout(() => {
        wsFlushTimerRef.current = null;
        flushPendingWsSnapshot();
      }, WS_FLUSH_INTERVAL_MS);
    }
  });

  useEffect(() => {
    if (!latest?.terms?.length) {
      setSelectedTermId(undefined);
      return;
    }
    if (!selectedTermId || !latest.terms.some((term) => term.id === selectedTermId)) {
      setSelectedTermId(latest.terms[0].id);
    }
  }, [latest, selectedTermId]);

  const selectedTerm = useMemo(
    () => latest?.terms?.find((term) => term.id === selectedTermId),
    [latest, selectedTermId]
  );
  const previousSnapshot = previousSnapshotRef.current;

  const handleManualRefresh = useCallback(async () => {
    if (manualRefreshing) return;
    setManualRefreshing(true);
    try {
      await Promise.all([
        fetchLatest(symbol, profile),
        fetchConfig(symbol, profile),
      ]);
    } finally {
      setManualRefreshing(false);
    }
  }, [fetchConfig, fetchLatest, manualRefreshing, profile, symbol]);

  const profileOptions = useMemo(
    () => (profiles.length ? profiles : [DEFAULT_FV_PROFILE]).map((item) => ({ label: item, value: item })),
    [profiles]
  );
  const symbolOptions = useMemo(
    () =>
      symbols.length
        ? symbols.map((item) => ({ label: item, value: item }))
        : [{ label: 'No symbols', value: '__none__', disabled: true }],
    [symbols]
  );

  const headerActions = (
    <div className="flex items-center gap-2">
      {symbol && (
        <Badge variant="neutral" size="xs">
          {symbol}
        </Badge>
      )}
      <Switch size="sm" checked={auto} onCheckedChange={setAuto} label="Auto" />
      <Select
        size="xs"
        value={profile}
        options={profileOptions}
        onChange={(value) => setProfile(value || DEFAULT_FV_PROFILE)}
      />
      <Select
        size="xs"
        value={symbol || '__none__'}
        disabled={!symbols.length}
        options={symbolOptions}
        onChange={(value) => setSymbol(value === '__none__' ? undefined : value)}
      />
    </div>
  );

  const content = (
    <div className={cn("flex h-full flex-col overflow-hidden", className)}>
      {showHeader && (
        <PanelHeader
          title="Fair Value"
          onRefresh={handleManualRefresh}
          refreshing={manualRefreshing}
          lastUpdate={lastUpdate}
          staleThresholdMs={STALE_THRESHOLDS.FAST}
          onRemove={onRemove}
          actions={headerActions}
        />
      )}
      {!showHeader && (
        <div className="flex items-center justify-end border-b border-border bg-bg-surface px-3 py-2">
          {headerActions}
        </div>
      )}
      <PanelBody className="bg-bg-base">
        <div className={cn("flex flex-col gap-5", dense ? "p-4" : "p-6")}>
          {error && (
            <div className="rounded-md border border-danger-dark/60 bg-danger/10 p-4 text-sm text-danger-light">
              {error}
            </div>
          )}

          <FVTable snapshot={latest} />
          <FVWhatMoved
            whatMoved={latest?.what_moved}
            currentSnapshot={latest}
            previousSnapshot={previousSnapshot}
          />
          <FVSpecSummary
            snapshot={latest}
            config={config}
            loading={configLoading}
            error={configError}
          />
          <FVBreakdown terms={latest?.terms || []} selectedTermId={selectedTermId} onSelectTerm={setSelectedTermId} />
          <FVFormulaInspector
            terms={latest?.terms || []}
            selectedTermId={selectedTerm?.id}
            onSelectTerm={setSelectedTermId}
            snapshotTrigger={latest?.trigger}
            snapshotTsMs={latest?.ts_ms}
            config={config}
          />
        </div>
      </PanelBody>
    </div>
  );

  if (showHeader) {
    return <PageShell>{content}</PageShell>;
  }
  return content;
}
