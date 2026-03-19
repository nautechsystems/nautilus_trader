import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { render, screen, waitFor } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';

import ScannersHarness from '../../pages/ScannersHarness';
import {
  REALTIME_BENCHMARK_SCENARIOS,
  REALTIME_BUDGETS,
  evaluateRealtimeBudgetStatus,
  runRealtimeBenchmark,
} from '../../components/trades/PerfHarness';

vi.mock('@/stores/scannersStore', () => ({
  useScannersStore: (selector: (state: Record<string, unknown>) => unknown) => selector({
    stats: {
      updatesPerSec: 0,
      applyDurationP50Ms: 0,
      applyDurationP95Ms: 0,
      renderDurationP95Ms: 0,
      deltaBufferSize: 0,
      totalRows: 0,
      droppedDeltas: 0,
    },
    enqueueDelta: vi.fn(),
    loadInitial: vi.fn(),
    refresh: vi.fn(),
  }),
}));

const EXPECTED_BUDGETS = {
  maxMountedRows: 120,
  maxBatchApplyCommitMs: 16,
  maxMultiPanelApplyCommitMs: 24,
  maxFreshnessLagMs: 1500,
  maxSelectorInvalidationsPerBatch: 50,
  maxRowRerendersPerDelta: 12,
  maxSteadyStateSnapshotRefreshesPerMinute: 2,
  maxPerCellTimers: 0,
} as const;

const EXPECTED_BENCHMARK_RESULTS = [
  {
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
  {
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
  {
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
] as const;

describe('realtime rollout budget baselines', () => {
  it('exports the committed rollout budgets and approval scenarios', () => {
    expect(REALTIME_BUDGETS).toEqual(EXPECTED_BUDGETS);
    expect(REALTIME_BENCHMARK_SCENARIOS).toEqual([
      'signal-live-500-rows',
      'trades-live-2000-rows',
      'signal-plus-trades-live',
    ]);
  });

  it('provides committed benchmark fixtures for every approval scenario', async () => {
    const results = await Promise.all(
      REALTIME_BENCHMARK_SCENARIOS.map((scenario) => runRealtimeBenchmark(scenario)),
    );

    expect(results).toEqual(EXPECTED_BENCHMARK_RESULTS);

    for (const result of results) {
      const status = evaluateRealtimeBudgetStatus(result);
      expect(status.pass).toBe(true);
      expect(status.checks).toHaveLength(7);
      expect(status.checks.every((check) => check.pass)).toBe(true);
    }
  });

  it('renders the same budgets and committed baselines in the scanners harness', async () => {
    render(<ScannersHarness />);

    expect(screen.getByRole('heading', { name: 'Rollout Budgets' })).toBeInTheDocument();
    expect(screen.getByText('Mounted rows: ≤ 120')).toBeInTheDocument();
    expect(screen.getByText('Single-panel apply+commit p95: ≤ 16ms')).toBeInTheDocument();
    expect(screen.getByText('Multi-panel apply+commit p95: ≤ 24ms')).toBeInTheDocument();

    await waitFor(() => {
      expect(screen.getByRole('heading', { name: 'Committed Baseline' })).toBeInTheDocument();
      expect(screen.getByText('Signal table live, 500 rows')).toBeInTheDocument();
      expect(screen.getByText('Trades table live, 2,000 rows')).toBeInTheDocument();
      expect(screen.getByText('Signal plus trades live split view')).toBeInTheDocument();
    });

    expect(screen.getAllByText('Status: PASS')).toHaveLength(3);
    expect(screen.getByText('Apply+commit p95: 22.4ms')).toBeInTheDocument();
    expect(screen.getAllByText('Snapshot refreshes / minute: 2')).toHaveLength(2);
  });

  it('documents the approval scenarios and exact default verification commands without extra env flags', () => {
    const plan = readFileSync(
      resolve(process.cwd(), '../docs/plans/2026-03-19-fluxboard-performance-improvement-plan.md'),
      'utf8',
    );

    expect(plan).toContain('## Rollout Budget Contract');
    expect(plan).toContain('## Benchmark Scenarios Used For Approval');
    expect(plan).toContain(
      'pnpm --dir fluxboard exec vitest run __tests__/realtime/baseline-budgets.test.tsx __tests__/pnl-performance.test.tsx __tests__/panels/trades.perf.test.tsx',
    );
    expect(plan).toContain(
      'pnpm --dir fluxboard exec vitest run __tests__/realtime/baseline-budgets.test.tsx __tests__/pnl-performance.test.tsx __tests__/panels/trades.perf.test.tsx components/trades/TradesTable.test.tsx',
    );
    expect(plan).not.toContain('VITEST_FULL=1');
  });
});
