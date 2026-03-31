import { useCallback, useEffect, useMemo, useRef, useState } from 'react';

import { api } from './api';
import { INTERVALS } from './constants';
import { isRealtimeStandardEnabled } from './config/featureFlags';
import {
  usePolling,
  useStandardWebSocketSubscription,
  useWebSocket,
} from './hooks';
import {
  useRealtimeSurfaceController,
  createRealtimeSurfaceController,
} from './hooks/useRealtimeSurfaceController';
import { PanelBody } from './components/shared/PanelBody';
import { PanelHeader } from './components/shared/PanelHeader';
import { PageShell } from './components/layout/PageShell';
import {
  buildTokenMMBalancesViewModel,
  createDefaultTokenMMBalancesToolbarState,
  reconcileExpandedTokenMMBalanceIds,
} from './components/balances/tokenmmBalancesModel';
import { TokenMMBalancesStatusStrip } from './components/balances/TokenMMBalancesStatusStrip';
import { TokenMMBalancesSummary } from './components/balances/TokenMMBalancesSummary';
import { TokenMMBalancesToolbar } from './components/balances/TokenMMBalancesToolbar';
import { TokenMMBalancesTable } from './components/balances/TokenMMBalancesTable';
import {
  useBalancesStore,
  selectBalancesDegraded,
  selectBalancesLoading,
  selectBalancesRows,
  selectBalancesScopeStatus,
  selectBalancesTotals,
  shallow,
} from './stores';
import type {
  BalanceParentRow,
  BalancesPayload,
  BalanceScopeStatus,
  RealtimeSnapshotLineage,
  TokenMMBalancesToolbarState,
} from './types';

const REFRESH_MS = INTERVALS.BALANCES_POLL;
const FILTER_STORAGE_KEY = 'balances:tokenmm:filters:v4';
const EXPANDED_STORAGE_KEY = 'balances:tokenmm:expanded:v3';

const noopWebSocketSubscribe = () => () => {};

function getBalancesRealtimeLineage(payload: BalancesPayload): RealtimeSnapshotLineage | null {
  return payload.realtime ?? null;
}

function sameLineageIdentity(
  left: RealtimeSnapshotLineage | null,
  right: RealtimeSnapshotLineage | null,
): boolean {
  if (!left || !right) {
    return left === right;
  }
  return left.contract_version === right.contract_version
    && left.surface === right.surface
    && left.profile === right.profile
    && left.surface_query_key === right.surface_query_key
    && left.stream_id === right.stream_id
    && String(left.snapshot_revision) === String(right.snapshot_revision);
}

function isScopeStatusDegraded(scope: BalanceScopeStatus): boolean {
  const projection = scope.projection_status;
  if (!projection) {
    return false;
  }
  if (projection.healthy === false) {
    return true;
  }
  if (
    projection.last_attempt_ts_ms != null
    && projection.last_success_ts_ms != null
    && projection.stale_after_ms != null
    && projection.stale_after_ms > 0
  ) {
    return (projection.last_attempt_ts_ms - projection.last_success_ts_ms) > projection.stale_after_ms;
  }
  return false;
}

function getStoredFilters(): TokenMMBalancesToolbarState {
  if (typeof window === 'undefined') {
    return createDefaultTokenMMBalancesToolbarState();
  }

  try {
    const raw = window.localStorage.getItem(FILTER_STORAGE_KEY);
    if (!raw) {
      return createDefaultTokenMMBalancesToolbarState();
    }
    const parsed = JSON.parse(raw) as Partial<TokenMMBalancesToolbarState>;
    return {
      ...createDefaultTokenMMBalancesToolbarState(),
      ...parsed,
    };
  } catch {
    return createDefaultTokenMMBalancesToolbarState();
  }
}

function getStoredExpandedIds(): Set<string> {
  if (typeof window === 'undefined') {
    return new Set<string>();
  }

  try {
    const raw = window.localStorage.getItem(EXPANDED_STORAGE_KEY);
    if (!raw) {
      return new Set<string>();
    }
    const parsed = JSON.parse(raw);
    if (!Array.isArray(parsed)) {
      return new Set<string>();
    }
    return new Set(parsed.filter((value): value is string => typeof value === 'string'));
  } catch {
    return new Set<string>();
  }
}

function buildFallbackPayload(
  rows: BalanceParentRow[],
  generatedAt: string | undefined,
  degraded: boolean,
  scopeStatus: BalanceScopeStatus[],
): BalancesPayload {
  return {
    rows,
    total: rows.length,
    totals: {
      mv_raw: rows.reduce((sum, row) => sum + row.mv_raw, 0),
      mv_display: '$0.00',
    },
    generated_at: generatedAt ?? new Date().toISOString(),
    view: 'parents_only',
    degraded,
    scope_status: scopeStatus,
    risk_groups: [],
  };
}

export default function Balances({
  className = '',
  onRemove,
  showHeader = true,
}: {
  dense?: boolean;
  className?: string;
  onRemove?: () => void;
  showHeader?: boolean;
} = {}) {
  const storeRows = useBalancesStore(selectBalancesRows, shallow);
  const totals = useBalancesStore(selectBalancesTotals);
  const loading = useBalancesStore(selectBalancesLoading);
  const degraded = useBalancesStore(selectBalancesDegraded);
  const scopeStatus = useBalancesStore(selectBalancesScopeStatus, shallow);
  const generatedAt = useBalancesStore((state) => state.generatedAt);
  const setData = useBalancesStore((state) => state.setData);
  const setLoading = useBalancesStore((state) => state.setLoading);

  const balancesRealtimeStandardEnabled = useMemo(
    () => isRealtimeStandardEnabled('balances'),
    [],
  );
  const [pollingFallbackEnabled, setPollingFallbackEnabled] = useState(
    () => !balancesRealtimeStandardEnabled,
  );
  const [filters, setFilters] = useState<TokenMMBalancesToolbarState>(getStoredFilters);
  const [expandedParentIds, setExpandedParentIds] = useState<Set<string>>(getStoredExpandedIds);
  const [latestPayload, setLatestPayload] = useState<BalancesPayload | null>(null);
  const [standardLineage, setStandardLineage] = useState<RealtimeSnapshotLineage | null>(null);

  const initialRowsRef = useRef(storeRows);
  const balancesController = useMemo(
    () => createRealtimeSurfaceController<BalanceParentRow>({
      getRowId: (row) => row.id,
      initialRows: initialRowsRef.current,
      compareRows: (left, right) => (right.mv_raw ?? 0) - (left.mv_raw ?? 0),
    }),
    [],
  );
  const controllerState = useRealtimeSurfaceController(
    balancesController,
    (snapshot) => ({
      rows: snapshot.rows as BalanceParentRow[],
    }),
  );
  const rows = balancesRealtimeStandardEnabled ? controllerState.rows : storeRows;

  const abortRef = useRef<AbortController | null>(null);
  const inFlight = useRef(false);
  const pendingRefreshRef = useRef(false);
  const standardResumeSeqRef = useRef(0);

  useEffect(() => () => {
    balancesController.destroy();
  }, [balancesController]);

  useEffect(() => {
    if (typeof window === 'undefined') {
      return;
    }
    window.localStorage.setItem(FILTER_STORAGE_KEY, JSON.stringify(filters));
  }, [filters]);

  useEffect(() => {
    if (typeof window === 'undefined') {
      return;
    }
    window.localStorage.setItem(EXPANDED_STORAGE_KEY, JSON.stringify(Array.from(expandedParentIds)));
  }, [expandedParentIds]);

  const fetchBalances = useCallback(async function fetchBalancesImpl() {
    if (inFlight.current) {
      pendingRefreshRef.current = true;
      return;
    }
    inFlight.current = true;

    abortRef.current?.abort();
    const abortController = new AbortController();
    abortRef.current = abortController;

    setLoading(true);
    try {
      const payload = await api.getBalances(
        balancesRealtimeStandardEnabled ? { contractVersion: 2 } : undefined,
      );

      if (abortController.signal.aborted) {
        return;
      }

      const nextLineage = getBalancesRealtimeLineage(payload);
      standardResumeSeqRef.current = Math.max(
        0,
        typeof nextLineage?.last_seq === 'number' ? nextLineage.last_seq : 0,
      );
      setStandardLineage((previous) => (
        sameLineageIdentity(previous, nextLineage) ? previous : nextLineage
      ));
      if (balancesRealtimeStandardEnabled) {
        balancesController.applySnapshot(payload.rows);
      }
      setData(payload);
      setLatestPayload(payload);
      if (balancesRealtimeStandardEnabled) {
        setPollingFallbackEnabled(!nextLineage);
      }
    } catch (error) {
      if (error instanceof Error && error.name === 'AbortError') {
        return;
      }
      if (balancesRealtimeStandardEnabled) {
        setPollingFallbackEnabled(true);
      }
      console.error('[balances] Failed to load:', error);
    } finally {
      inFlight.current = false;
      if (!abortController.signal.aborted) {
        setLoading(false);
      }
      if (pendingRefreshRef.current) {
        pendingRefreshRef.current = false;
        void fetchBalancesImpl();
      }
    }
  }, [balancesController, balancesRealtimeStandardEnabled, setData, setLoading]);

  usePolling(fetchBalances, REFRESH_MS, pollingFallbackEnabled);

  useEffect(() => {
    if (balancesRealtimeStandardEnabled) {
      void fetchBalances();
    }
  }, [balancesRealtimeStandardEnabled, fetchBalances]);

  useStandardWebSocketSubscription({
    enabled: balancesRealtimeStandardEnabled && Boolean(standardLineage),
    lineage: balancesRealtimeStandardEnabled ? standardLineage : null,
    resumeFromSeq: () => (
      standardResumeSeqRef.current > 0
        ? standardResumeSeqRef.current
        : (standardLineage?.last_seq ?? 0)
    ),
    onEvent: (event) => {
      if (typeof event.seq === 'number' && Number.isFinite(event.seq)) {
        standardResumeSeqRef.current = Math.max(standardResumeSeqRef.current, event.seq);
      }
      if (event.kind === 'invalidate') {
        void fetchBalances();
      }
    },
    onFailure: () => {
      if (balancesRealtimeStandardEnabled) {
        setPollingFallbackEnabled(true);
      }
      void fetchBalances();
    },
    onSubscribed: (ack) => {
      const acceptedSeq = typeof ack.accepted_start_seq === 'number' ? ack.accepted_start_seq : 0;
      const lastSeq = typeof ack.last_seq === 'number' ? ack.last_seq : acceptedSeq;
      standardResumeSeqRef.current = Math.max(standardResumeSeqRef.current, acceptedSeq, lastSeq);
    },
  });

  useWebSocket(
    balancesRealtimeStandardEnabled ? '__balances_legacy_disabled__' : 'market_update',
    useCallback(() => {
      if (!balancesRealtimeStandardEnabled) {
        void fetchBalances();
      }
    }, [balancesRealtimeStandardEnabled, fetchBalances]),
    balancesRealtimeStandardEnabled
      ? { subscribe: noopWebSocketSubscribe }
      : { surface: 'balances' },
  );

  useEffect(() => {
    return () => {
      abortRef.current?.abort();
    };
  }, []);

  useEffect(() => {
    setExpandedParentIds((previous) => reconcileExpandedTokenMMBalanceIds(previous, rows));
  }, [rows]);

  const payloadForView = useMemo<BalancesPayload>(() => {
    const base = latestPayload ?? buildFallbackPayload(rows, generatedAt, degraded, scopeStatus);
    return {
      ...base,
      rows,
      totals: totals ?? base.totals,
      generated_at: generatedAt ?? base.generated_at,
      degraded,
      scope_status: scopeStatus,
    };
  }, [degraded, generatedAt, latestPayload, rows, scopeStatus, totals]);

  const viewModel = useMemo(
    () => buildTokenMMBalancesViewModel(payloadForView, filters, expandedParentIds),
    [payloadForView, filters, expandedParentIds],
  );

  const scopeDegradedCount = useMemo(
    () => (payloadForView.scope_status ?? []).filter((scope) => isScopeStatusDegraded(scope)).length,
    [payloadForView.scope_status],
  );

  const visibleParentIds = useMemo(
    () => [...viewModel.sections.stables, ...viewModel.sections.trading].map((row) => row.id),
    [viewModel.sections.stables, viewModel.sections.trading],
  );
  const allExpanded = visibleParentIds.length > 0 && visibleParentIds.every((id) => expandedParentIds.has(id));
  const hasVisibleRows = visibleParentIds.length > 0;

  const handleToggleExpanded = useCallback((id: string) => {
    setExpandedParentIds((previous) => {
      const next = new Set(previous);
      if (next.has(id)) {
        next.delete(id);
      } else {
        next.add(id);
      }
      return next;
    });
  }, []);

  const handleToggleExpandAll = useCallback(() => {
    setExpandedParentIds((previous) => {
      const next = new Set(previous);
      if (allExpanded) {
        visibleParentIds.forEach((id) => next.delete(id));
      } else {
        visibleParentIds.forEach((id) => next.add(id));
      }
      return next;
    });
  }, [allExpanded, visibleParentIds]);

  const handleManualRefresh = useCallback(() => {
    void fetchBalances();
  }, [fetchBalances]);

  const content = (
    <div className={`flex h-full flex-col overflow-hidden ${className}`.trim()}>
      {showHeader && (
        <PanelHeader
          title="Balances"
          onRefresh={handleManualRefresh}
          refreshing={loading}
          onRemove={onRemove}
        />
      )}
      <PanelBody className="flex-1 overflow-y-auto bg-bg-surface">
        <div className="flex flex-col gap-4 p-4">
          <TokenMMBalancesStatusStrip
            source={payloadForView.source ?? null}
            degraded={Boolean(payloadForView.degraded)}
            degradedScopeCount={scopeDegradedCount}
          />
          <TokenMMBalancesSummary summary={viewModel.summary} />
          <TokenMMBalancesToolbar
            filters={filters}
            venueOptions={viewModel.venueOptions}
            allExpanded={allExpanded}
            onSearchChange={(search) => setFilters((previous) => ({ ...previous, search }))}
            onVenueChange={(venue) => setFilters((previous) => ({ ...previous, venue }))}
            onTypeChange={(type) => setFilters((previous) => ({ ...previous, type }))}
            onHideZeroChange={(hideZero) => setFilters((previous) => ({ ...previous, hideZero }))}
            onToggleExpandAll={handleToggleExpandAll}
          />
          {loading && !hasVisibleRows ? (
            <div className="rounded border border-border bg-bg-base px-4 py-8 text-center text-sm text-text-muted">
              Loading balances...
            </div>
          ) : null}
          {!loading && !hasVisibleRows ? (
            <div className="rounded border border-border bg-bg-base px-4 py-8 text-center text-sm text-text-muted">
              No balances found
            </div>
          ) : null}
          {hasVisibleRows ? (
            <>
              <TokenMMBalancesTable
                sectionTitle="Stables"
                rows={viewModel.sections.stables}
                expandedParentIds={expandedParentIds}
                onToggleExpanded={handleToggleExpanded}
              />
              <TokenMMBalancesTable
                sectionTitle="Trading Assets"
                rows={viewModel.sections.trading}
                expandedParentIds={expandedParentIds}
                onToggleExpanded={handleToggleExpanded}
              />
            </>
          ) : null}
        </div>
      </PanelBody>
    </div>
  );

  return (
    <PageShell className="h-full flex flex-col overflow-hidden">
      {content}
    </PageShell>
  );
}
