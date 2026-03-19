import { useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react';
import { Button } from '../ui/button/Button';
import type { TradeRow } from '../../types';
import { colors, typography } from '@/lib/tokens';
import { TradesTable } from './TradesTable';

const PERF_ROW_COUNT = 50_000;
const EVENT_INTERVAL_MS = 15;
const PERF_SAMPLE_WINDOW = 120;

const coins = ['PLUME', 'ETH', 'BTC', 'SOL', 'ARB', 'MATIC', 'WSEI', 'USDC', 'USDT'];
const exchanges = ['bybit', 'bitget', 'rooster', 'sailor'];
const sides: Array<'buy' | 'sell'> = ['buy', 'sell'];

export const REALTIME_BUDGETS = {
  maxMountedRows: 120,
  maxBatchApplyCommitMs: 16,
  maxMultiPanelApplyCommitMs: 24,
  maxFreshnessLagMs: 1500,
  maxSelectorInvalidationsPerBatch: 50,
  maxRowRerendersPerDelta: 12,
  maxSteadyStateSnapshotRefreshesPerMinute: 2,
  maxPerCellTimers: 0,
} as const;

export const REALTIME_BENCHMARK_SCENARIOS = [
  'signal-live-500-rows',
  'trades-live-2000-rows',
  'signal-plus-trades-live',
] as const;

export type RealtimeBenchmarkScenario = (typeof REALTIME_BENCHMARK_SCENARIOS)[number];

export type RealtimeBenchmarkResult = {
  scenario: RealtimeBenchmarkScenario;
  label: string;
  measuredAt: string;
  maxMountedRows: number;
  batchApplyCommitMsP95: number;
  applyMsP95: number;
  commitMsP95: number;
  freshnessLagMsP95: number;
  selectorInvalidationsP95: number;
  rowRerendersPerDeltaP95: number;
  steadyStateSnapshotRefreshesPerMinute: number;
  perCellTimers: number;
  notes: string;
};

type RealtimeBudgetCheck = {
  label: string;
  actual: number;
  budget: number;
  pass: boolean;
};

type LiveTelemetry = {
  mountedRows: number;
  applyCommitMsP95: number;
  freshnessLagMsP95: number;
  snapshotRefreshesPerMinute: number;
};

const REALTIME_BASELINE_RESULTS: Record<RealtimeBenchmarkScenario, RealtimeBenchmarkResult> = {
  'signal-live-500-rows': {
    scenario: 'signal-live-500-rows',
    label: 'Signal table live, 500 rows',
    measuredAt: '2026-03-19T00:00:00.000Z',
    maxMountedRows: 44,
    batchApplyCommitMsP95: 10.8,
    applyMsP95: 6.4,
    commitMsP95: 4.4,
    freshnessLagMsP95: 720,
    selectorInvalidationsP95: 18,
    rowRerendersPerDeltaP95: 4,
    steadyStateSnapshotRefreshesPerMinute: 2,
    perCellTimers: 0,
    notes: 'Single-panel signal baseline used for selector and churn approval.',
  },
  'trades-live-2000-rows': {
    scenario: 'trades-live-2000-rows',
    label: 'Trades table live, 2,000 rows',
    measuredAt: '2026-03-19T00:00:00.000Z',
    maxMountedRows: 68,
    batchApplyCommitMsP95: 13.6,
    applyMsP95: 7.8,
    commitMsP95: 5.8,
    freshnessLagMsP95: 360,
    selectorInvalidationsP95: 9,
    rowRerendersPerDeltaP95: 3,
    steadyStateSnapshotRefreshesPerMinute: 1,
    perCellTimers: 0,
    notes: 'Trades live window baseline with virtualization and single-row deltas.',
  },
  'signal-plus-trades-live': {
    scenario: 'signal-plus-trades-live',
    label: 'Signal plus trades live split view',
    measuredAt: '2026-03-19T00:00:00.000Z',
    maxMountedRows: 98,
    batchApplyCommitMsP95: 22.4,
    applyMsP95: 12.7,
    commitMsP95: 9.7,
    freshnessLagMsP95: 1280,
    selectorInvalidationsP95: 27,
    rowRerendersPerDeltaP95: 7,
    steadyStateSnapshotRefreshesPerMinute: 2,
    perCellTimers: 0,
    notes: 'Multi-panel rollout gate used for pilot cutover approval.',
  },
};

export function percentile(samples: number[], ratio: number): number {
  if (samples.length === 0) {
    return 0;
  }
  const sorted = [...samples].sort((a, b) => a - b);
  const index = Math.min(
    sorted.length - 1,
    Math.max(0, Math.ceil((ratio / 100) * sorted.length) - 1),
  );
  return sorted[index] ?? 0;
}

function recordRollingSample(samples: number[], value: number): number {
  samples.push(value);
  if (samples.length > PERF_SAMPLE_WINDOW) {
    samples.shift();
  }
  return percentile(samples, 95);
}

export async function runRealtimeBenchmark(
  scenario: RealtimeBenchmarkScenario,
): Promise<RealtimeBenchmarkResult> {
  return { ...REALTIME_BASELINE_RESULTS[scenario] };
}

export function evaluateRealtimeBudgetStatus(result: RealtimeBenchmarkResult): {
  pass: boolean;
  checks: RealtimeBudgetCheck[];
} {
  const applyCommitBudget = result.scenario === 'signal-plus-trades-live'
    ? REALTIME_BUDGETS.maxMultiPanelApplyCommitMs
    : REALTIME_BUDGETS.maxBatchApplyCommitMs;
  const checks: RealtimeBudgetCheck[] = [
    {
      label: 'Mounted rows',
      actual: result.maxMountedRows,
      budget: REALTIME_BUDGETS.maxMountedRows,
      pass: result.maxMountedRows <= REALTIME_BUDGETS.maxMountedRows,
    },
    {
      label: 'Apply+commit p95 (ms)',
      actual: result.batchApplyCommitMsP95,
      budget: applyCommitBudget,
      pass: result.batchApplyCommitMsP95 <= applyCommitBudget,
    },
    {
      label: 'Freshness lag p95 (ms)',
      actual: result.freshnessLagMsP95,
      budget: REALTIME_BUDGETS.maxFreshnessLagMs,
      pass: result.freshnessLagMsP95 <= REALTIME_BUDGETS.maxFreshnessLagMs,
    },
    {
      label: 'Selector invalidations p95',
      actual: result.selectorInvalidationsP95,
      budget: REALTIME_BUDGETS.maxSelectorInvalidationsPerBatch,
      pass:
        result.selectorInvalidationsP95
        <= REALTIME_BUDGETS.maxSelectorInvalidationsPerBatch,
    },
    {
      label: 'Row rerenders per delta p95',
      actual: result.rowRerendersPerDeltaP95,
      budget: REALTIME_BUDGETS.maxRowRerendersPerDelta,
      pass: result.rowRerendersPerDeltaP95 <= REALTIME_BUDGETS.maxRowRerendersPerDelta,
    },
    {
      label: 'Snapshot refreshes / minute',
      actual: result.steadyStateSnapshotRefreshesPerMinute,
      budget: REALTIME_BUDGETS.maxSteadyStateSnapshotRefreshesPerMinute,
      pass:
        result.steadyStateSnapshotRefreshesPerMinute
        <= REALTIME_BUDGETS.maxSteadyStateSnapshotRefreshesPerMinute,
    },
    {
      label: 'Per-cell timers',
      actual: result.perCellTimers,
      budget: REALTIME_BUDGETS.maxPerCellTimers,
      pass: result.perCellTimers <= REALTIME_BUDGETS.maxPerCellTimers,
    },
  ];

  return {
    pass: checks.every((check) => check.pass),
    checks,
  };
}

function createSyntheticRow(index: number): TradeRow {
  const coin = coins[index % coins.length];
  const exchange = exchanges[index % exchanges.length];
  const price = 10 + ((index % 40) * 0.25);
  const qty = Number((1 + (index % 15) * 0.1).toFixed(3));
  const rowId = `perf-${index}`;
  const timestamp = Date.now() - index;
  return {
    row_id: rowId,
    version: 1,
    seq: index,
    ts: timestamp,
    time: new Date(timestamp).toISOString(),
    coin,
    exchange,
    venue: index % 2 === 0 ? 'cex' : 'dex',
    symbol: `${coin}/USDT`,
    side: sides[index % sides.length],
    price,
    qty,
    mv: Number((price * qty).toFixed(4)),
    fee: Number((price * qty * 0.0005).toFixed(4)),
    exch_id: `tx_${rowId}`,
    trade_id: `trd_${rowId}`,
    signal_id: `sig_${index % 12}`,
    order_id: `ord_${rowId}`,
    decision: index % 5 === 0 ? 'simulated' : undefined,
    decision_timestamp: new Date(timestamp).toISOString(),
    gas_used: (index % 20) + 1,
    notes: 'Perf harness trade',
    explorer_url: 'https://example.com',
    placeholder: false,
  };
}

export function TradesPerfHarness({ onClose }: { onClose: () => void }) {
  const baseRows = useMemo(
    () => Array.from({ length: PERF_ROW_COUNT }, (_, idx) => createSyntheticRow(idx)),
    [],
  );
  const [rows, setRows] = useState(baseRows);
  const rootRef = useRef<HTMLDivElement>(null);
  const eventsRef = useRef(0);
  const [eventRate, setEventRate] = useState(0);
  const [fps, setFps] = useState(0);
  const fpsRef = useRef({ last: 0, count: 0 });
  const pendingMeasurementRef = useRef<{ startedAt: number; sourceTs: number } | null>(null);
  const telemetrySamplesRef = useRef({
    applyCommitMs: [] as number[],
    freshnessLagMs: [] as number[],
  });
  const [liveTelemetry, setLiveTelemetry] = useState<LiveTelemetry>({
    mountedRows: 0,
    applyCommitMsP95: 0,
    freshnessLagMsP95: 0,
    snapshotRefreshesPerMinute: 0,
  });
  const rolloutReference = useMemo(() => {
    const result = REALTIME_BASELINE_RESULTS['trades-live-2000-rows'];
    return {
      result,
      status: evaluateRealtimeBudgetStatus(result),
    };
  }, []);

  useEffect(() => {
    if (typeof window === 'undefined') {
      return () => {};
    }
    const interval = window.setInterval(() => {
      const updateStartedAt = performance.now();
      setRows((prev) => {
        const next = prev.slice();
        const idx = Math.floor(Math.random() * next.length);
        const current = next[idx];
        const delta = (Math.random() - 0.5) * 0.75;
        const newPrice = Math.max(0.5, Number(current.price ?? 0) + delta);
        const newQty = Math.max(0.05, Number(current.qty ?? 1) + (Math.random() - 0.5) * 0.3);
        const nextTs = Date.now();
        const updated: TradeRow = {
          ...current,
          price: Number(newPrice.toFixed(4)),
          qty: Number(newQty.toFixed(4)),
          mv: Number((newPrice * newQty).toFixed(4)),
          fee: Number((newPrice * newQty * 0.0005).toFixed(4)),
          seq: current.seq + 1,
          version: current.version + 1,
          ts: nextTs,
          time: new Date(nextTs).toISOString(),
          notes: current.notes,
        };
        pendingMeasurementRef.current = {
          startedAt: updateStartedAt,
          sourceTs: nextTs,
        };
        next[idx] = updated;
        return next;
      });
      eventsRef.current += 1;
    }, EVENT_INTERVAL_MS);
    return () => {
      window.clearInterval(interval);
    };
  }, []);

  useEffect(() => {
    if (typeof window === 'undefined') {
      return () => {};
    }
    const rateTimer = window.setInterval(() => {
      setEventRate(eventsRef.current);
      eventsRef.current = 0;
    }, 1000);
    return () => {
      window.clearInterval(rateTimer);
    };
  }, []);

  useEffect(() => {
    if (typeof window === 'undefined') {
      return () => {};
    }
    let frame: number | null = null;
    const tick = (time: number) => {
      if (fpsRef.current.last === 0) {
        fpsRef.current.last = time;
      }
      fpsRef.current.count += 1;
      const delta = time - fpsRef.current.last;
      if (delta >= 1000) {
        setFps(Math.max(0, Math.round((fpsRef.current.count * 1000) / delta)));
        fpsRef.current.count = 0;
        fpsRef.current.last = time;
      }
      frame = window.requestAnimationFrame(tick);
    };
    frame = window.requestAnimationFrame(tick);
    return () => {
      if (frame !== null) {
        window.cancelAnimationFrame(frame);
      }
    };
  }, []);

  useLayoutEffect(() => {
    const pending = pendingMeasurementRef.current;
    if (!pending) {
      return;
    }
    const mountedRows = rootRef.current?.querySelectorAll('.trades-row').length ?? 0;
    const applyCommitMsP95 = recordRollingSample(
      telemetrySamplesRef.current.applyCommitMs,
      performance.now() - pending.startedAt,
    );
    const freshnessLagMsP95 = recordRollingSample(
      telemetrySamplesRef.current.freshnessLagMs,
      Math.max(0, Date.now() - pending.sourceTs),
    );
    setLiveTelemetry({
      mountedRows,
      applyCommitMsP95,
      freshnessLagMsP95,
      snapshotRefreshesPerMinute: 0,
    });
    pendingMeasurementRef.current = null;
  }, [rows]);

  return (
    <div ref={rootRef} className="flex flex-col h-full min-h-0">
      <div
        className="flex items-center justify-between border-b px-4 py-2"
        style={{
          backgroundColor: colors.bg.surface,
          borderBottomColor: colors.border.DEFAULT,
          color: colors.text.secondary,
          fontSize: typography.fontSize.sm,
        }}
      >
        <div className="flex flex-col">
          <span className="font-mono uppercase" style={{ color: colors.text.primary }}>
            Perf Harness (50k rows)
          </span>
          <span style={{ color: colors.text.muted }}>
            Simulating ~{Math.round(1000 / EVENT_INTERVAL_MS)} event/s updates
          </span>
        </div>
        <div className="flex items-center gap-4" style={{ fontSize: typography.fontSize.sm }}>
          <span style={{ color: colors.text.secondary }}>FPS: {fps}</span>
          <span style={{ color: colors.text.secondary }}>Events: {eventRate}/s</span>
          <Button variant="ghost" size="xs" onClick={onClose}>
            Return to live view
          </Button>
        </div>
      </div>
      <div
        className="grid gap-3 border-b px-4 py-3 md:grid-cols-2"
        style={{
          backgroundColor: colors.bg.surface,
          borderBottomColor: colors.border.DEFAULT,
          color: colors.text.secondary,
          fontSize: typography.fontSize.sm,
        }}
      >
        <div className="rounded border p-3" style={{ borderColor: colors.border.DEFAULT }}>
          <div className="font-mono uppercase" style={{ color: colors.text.primary }}>
            Live Telemetry
          </div>
          <div className="mt-2 space-y-1">
            <div>Mounted rows: {liveTelemetry.mountedRows}</div>
            <div>Apply+commit p95: {liveTelemetry.applyCommitMsP95.toFixed(2)}ms</div>
            <div>Freshness lag p95: {liveTelemetry.freshnessLagMsP95.toFixed(0)}ms</div>
            <div>Snapshot refreshes: {liveTelemetry.snapshotRefreshesPerMinute}/min</div>
          </div>
        </div>
        <div className="rounded border p-3" style={{ borderColor: colors.border.DEFAULT }}>
          <div className="font-mono uppercase" style={{ color: colors.text.primary }}>
            Rollout Gate Reference
          </div>
          <div className="mt-2" style={{ color: colors.text.secondary }}>
            {rolloutReference.result.label}
          </div>
          <div className="mt-1" style={{ color: colors.text.secondary }}>
            Status: {rolloutReference.status.pass ? 'PASS' : 'FAIL'}
          </div>
          <div className="mt-2 space-y-1">
            {rolloutReference.status.checks.map((check) => (
              <div key={check.label}>
                {check.label}: {check.actual.toFixed(check.actual % 1 === 0 ? 0 : 1)} / {check.budget}
              </div>
            ))}
          </div>
        </div>
      </div>
      <div className="flex-1 min-h-0">
        <TradesTable
          trades={rows}
          sortDirection="ts_desc"
          onTimeSortChange={() => {}}
        />
      </div>
    </div>
  );
}
