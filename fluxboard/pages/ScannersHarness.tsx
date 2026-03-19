/**
 * Scanners Performance Harness - Dev-only testing tool
 *
 * Loads scenario JSON files and injects synthetic deltas into the scanners store
 * to test performance under various load conditions.
 *
 * Access via: /scanners-harness (dev mode only)
 */

import { useState, useEffect, useRef } from 'react';
import { useScannersStore } from '@/stores/scannersStore';
import type { ScannerPricingSnapshot, ScannerPricingDelta } from '@/types';
import {
  REALTIME_BENCHMARK_SCENARIOS,
  REALTIME_BUDGETS,
  evaluateRealtimeBudgetStatus,
  runRealtimeBenchmark,
} from '../components/trades/PerfHarness';

type Scenario = {
  name: string;
  description: string;
  base_rows: number;
  delta_rate_per_sec: number;
  duration_sec: number;
};

type HarnessStats = {
  deltasInjected: number;
  startTime: number;
  endTime: number | null;
  fps: number[];
  commitDurations: number[];
};

type RolloutBaseline = Awaited<ReturnType<typeof runRealtimeBenchmark>>;

const SCENARIOS: Scenario[] = [
  { name: '5k @ 100Hz', base_rows: 5000, delta_rate_per_sec: 100, duration_sec: 60, description: '5k rows, 100 updates/sec' },
  { name: '10k @ 100Hz', base_rows: 10000, delta_rate_per_sec: 100, duration_sec: 60, description: '10k rows, 100 updates/sec' },
  { name: '10k @ 300Hz', base_rows: 10000, delta_rate_per_sec: 300, duration_sec: 60, description: '10k rows, 300 updates/sec' },
];

export default function ScannersHarness() {
  const [selectedScenario, setSelectedScenario] = useState<Scenario | null>(null);
  const [running, setRunning] = useState(false);
  const [rolloutBaselines, setRolloutBaselines] = useState<RolloutBaseline[]>([]);
  const [stats, setStats] = useState<HarnessStats>({
    deltasInjected: 0,
    startTime: 0,
    endTime: null,
    fps: [],
    commitDurations: [],
  });

  const storeStats = useScannersStore((state) => state.stats);
  const enqueueDelta = useScannersStore((state) => state.enqueueDelta);
  const loadInitial = useScannersStore((state) => state.loadInitial);
  const refresh = useScannersStore((state) => state.refresh);

  const intervalRef = useRef<number | null>(null);
  const fpsRef = useRef<number[]>([]);
  const lastFrameTime = useRef<number>(performance.now());

  useEffect(() => {
    let active = true;

    Promise.all(REALTIME_BENCHMARK_SCENARIOS.map((scenario) => runRealtimeBenchmark(scenario)))
      .then((results) => {
        if (active) {
          setRolloutBaselines(results);
        }
      })
      .catch(() => {
        if (active) {
          setRolloutBaselines([]);
        }
      });

    return () => {
      active = false;
    };
  }, []);

  // FPS tracking
  useEffect(() => {
    if (!running) return;

    const measureFPS = () => {
      const now = performance.now();
      const elapsed = now - lastFrameTime.current;
      if (elapsed > 0) {
        fpsRef.current.push(1000 / elapsed);
        if (fpsRef.current.length > 60) {
          fpsRef.current.shift();
        }
      }
      lastFrameTime.current = now;
      requestAnimationFrame(measureFPS);
    };

    const rafId = requestAnimationFrame(measureFPS);
    return () => cancelAnimationFrame(rafId);
  }, [running]);

  const generateSnapshot = (poolIndex: number): ScannerPricingSnapshot => {
    const poolAddress = `0x${poolIndex.toString(16).padStart(40, '0')}`;
    const tokens = ['WBNB', 'USDT', 'WETH', 'BTC', 'PLUME', 'WPLUME'];
    const token0 = tokens[poolIndex % tokens.length];
    const token1 = tokens[(poolIndex + 1) % tokens.length];
    const dexMid = 500 + (poolIndex % 1000);
    const bestEdge = 10 + (poolIndex % 100);

    return {
      scanner_id: 'pcs_bnb_usdt',
      pool_address: poolAddress,
      token0,
      token1,
      dex_name: 'pancakeswap_v3',
      chain: 'bnb',
      dex_mid: String(dexMid),
      cex_bid: String(dexMid * 0.999),
      cex_ask: String(dexMid * 1.001),
      best_edge_bps: String(bestEdge),
      net_edge_sell_dex_bps: String(Math.max(0, bestEdge - 5)),
      net_edge_buy_dex_bps: String(Math.max(0, bestEdge - 5)),
      tvl_usd: String(100000 + (poolIndex % 1000000)),
      volume_24h_usd: String(500000 + (poolIndex % 5000000)),
      bybit_marginable: poolIndex % 3 === 0,
      bybit_symbol: `${token0}/USDT`,
      last_update_ts: Date.now() - (poolIndex % 5000),
      cex_last_update_ts: Date.now() - (poolIndex % 3000),
      dex_last_update_ts: Date.now() - (poolIndex % 2000),
      dex_fee_bps: String(10 + (poolIndex % 20)),
      cex_fee_bps: String(15 + (poolIndex % 10)),
      cex_fee_effective_bps: String(15 + (poolIndex % 10)),
      cex_fee_sell_path_bps: String(15 + (poolIndex % 10)),
      cex_fee_buy_path_bps: String(15 + (poolIndex % 10)),
      best_direction: poolIndex % 2 === 0 ? 'sell_dex_buy_cex' : 'buy_dex_sell_cex',
    } as ScannerPricingSnapshot;
  };

  const runScenario = async (scenario: Scenario) => {
    setRunning(true);
    setStats({
      deltasInjected: 0,
      startTime: Date.now(),
      endTime: null,
      fps: [],
      commitDurations: [],
    });

    // Reset store and load base snapshots
    refresh();
    await new Promise(resolve => setTimeout(resolve, 500));

    // Inject base snapshots
    const baseSnapshots: ScannerPricingSnapshot[] = [];
    for (let i = 0; i < scenario.base_rows; i++) {
      baseSnapshots.push(generateSnapshot(i));
    }

    // Inject base snapshots via API simulation (would need store method for this)
    // For now, we'll inject them as deltas

    const interval = 1000 / scenario.delta_rate_per_sec;
    const endTime = Date.now() + (scenario.duration_sec * 1000);
    let deltaCount = 0;

    const injectDelta = () => {
      if (Date.now() >= endTime) {
        if (intervalRef.current) {
          clearInterval(intervalRef.current);
          intervalRef.current = null;
        }
        setRunning(false);
        setStats(prev => ({
          ...prev,
          endTime: Date.now(),
          fps: [...fpsRef.current],
        }));
        return;
      }

      // Pick random snapshot and update it
      const poolIndex = Math.floor(Math.random() * scenario.base_rows);
      const snapshot = generateSnapshot(poolIndex);
      snapshot.best_edge_bps = String(parseFloat(snapshot.best_edge_bps || '0') + (Math.random() * 10 - 5));
      snapshot.last_update_ts = Date.now();

      const delta: ScannerPricingDelta = {
        scanner_id: 'pcs_bnb_usdt',
        pool_address: snapshot.pool_address!,
        fields_changed: ['best_edge_bps', 'last_update_ts'],
        snapshot,
      };

      enqueueDelta(delta);
      deltaCount++;

      setStats(prev => ({
        ...prev,
        deltasInjected: deltaCount,
      }));
    };

    intervalRef.current = window.setInterval(injectDelta, interval);
  };

  const stopScenario = () => {
    if (intervalRef.current) {
      clearInterval(intervalRef.current);
      intervalRef.current = null;
    }
    setRunning(false);
    setStats(prev => ({
      ...prev,
      endTime: Date.now(),
      fps: [...fpsRef.current],
    }));
  };

  const avgFPS = stats.fps.length > 0
    ? stats.fps.reduce((a, b) => a + b, 0) / stats.fps.length
    : 0;
  const minFPS = stats.fps.length > 0 ? Math.min(...stats.fps) : 0;

  const elapsed = stats.endTime
    ? (stats.endTime - stats.startTime) / 1000
    : stats.startTime > 0
      ? (Date.now() - stats.startTime) / 1000
      : 0;

  return (
    <div className="p-6 max-w-4xl mx-auto">
      <h1 className="text-2xl font-bold mb-4">Scanners Performance Harness</h1>

      <div className="mb-6">
        <label className="block mb-2">Select Scenario:</label>
        <select
          value={selectedScenario?.name || ''}
          onChange={(e) => {
            const scenario = SCENARIOS.find(s => s.name === e.target.value);
            setSelectedScenario(scenario || null);
          }}
          disabled={running}
          className="px-3 py-2 border rounded"
        >
          <option value="">-- Select --</option>
          {SCENARIOS.map(s => (
            <option key={s.name} value={s.name}>{s.name}: {s.description}</option>
          ))}
        </select>
      </div>

      <div className="mb-6 flex gap-4">
        <button
          onClick={() => selectedScenario && runScenario(selectedScenario)}
          disabled={running || !selectedScenario}
          className="px-4 py-2 bg-blue-600 text-white rounded disabled:opacity-50"
        >
          Start Scenario
        </button>
        <button
          onClick={stopScenario}
          disabled={!running}
          className="px-4 py-2 bg-red-600 text-white rounded disabled:opacity-50"
        >
          Stop
        </button>
      </div>

      <div className="grid grid-cols-1 gap-4 mb-6 md:grid-cols-2">
        <div className="p-4 border rounded">
          <h3 className="font-semibold mb-2">Rollout Budgets</h3>
          <p>Mounted rows: ≤ {REALTIME_BUDGETS.maxMountedRows}</p>
          <p>Single-panel apply+commit p95: ≤ {REALTIME_BUDGETS.maxBatchApplyCommitMs}ms</p>
          <p>Multi-panel apply+commit p95: ≤ {REALTIME_BUDGETS.maxMultiPanelApplyCommitMs}ms</p>
          <p>Freshness lag p95: ≤ {REALTIME_BUDGETS.maxFreshnessLagMs}ms</p>
          <p>Selector invalidations p95: ≤ {REALTIME_BUDGETS.maxSelectorInvalidationsPerBatch}</p>
          <p>Row rerenders per delta p95: ≤ {REALTIME_BUDGETS.maxRowRerendersPerDelta}</p>
          <p>
            Snapshot refreshes / minute: ≤ {REALTIME_BUDGETS.maxSteadyStateSnapshotRefreshesPerMinute}
          </p>
          <p>Per-cell timers: {REALTIME_BUDGETS.maxPerCellTimers}</p>
        </div>

        <div className="p-4 border rounded">
          <h3 className="font-semibold mb-2">Committed Baseline</h3>
          {rolloutBaselines.length === 0 ? (
            <p>Loading benchmark baselines...</p>
          ) : (
            rolloutBaselines.map((baseline) => {
              const status = evaluateRealtimeBudgetStatus(baseline);
              return (
                <div key={baseline.scenario} className="mb-3 last:mb-0">
                  <p className="font-medium">{baseline.label}</p>
                  <p>Status: {status.pass ? 'PASS' : 'FAIL'}</p>
                  <p>Mounted rows: {baseline.maxMountedRows}</p>
                  <p>Apply+commit p95: {baseline.batchApplyCommitMsP95.toFixed(1)}ms</p>
                  <p>Freshness lag p95: {baseline.freshnessLagMsP95.toFixed(0)}ms</p>
                  <p>Selector invalidations p95: {baseline.selectorInvalidationsP95}</p>
                  <p>Row rerenders per delta p95: {baseline.rowRerendersPerDeltaP95}</p>
                  <p>
                    Snapshot refreshes / minute: {baseline.steadyStateSnapshotRefreshesPerMinute}
                  </p>
                </div>
              );
            })
          )}
        </div>
      </div>

      {running && (
        <div className="mb-6 p-4 bg-yellow-100 rounded">
          <p>Running scenario: {selectedScenario?.name}</p>
          <p>Elapsed: {elapsed.toFixed(1)}s</p>
        </div>
      )}

      <div className="grid grid-cols-2 gap-4 mb-6">
        <div className="p-4 border rounded">
          <h3 className="font-semibold mb-2">Harness Stats</h3>
          <p>Deltas Injected: {stats.deltasInjected}</p>
          <p>Elapsed: {elapsed.toFixed(1)}s</p>
          <p>Avg FPS: {avgFPS.toFixed(1)}</p>
          <p>Min FPS: {minFPS.toFixed(1)}</p>
        </div>

        <div className="p-4 border rounded">
          <h3 className="font-semibold mb-2">Store Stats</h3>
          <p>Updates/sec: {storeStats.updatesPerSec}</p>
          <p>Apply p50: {storeStats.applyDurationP50Ms.toFixed(1)}ms</p>
          <p>Apply p95: {storeStats.applyDurationP95Ms.toFixed(1)}ms</p>
          <p>Render p95: {storeStats.renderDurationP95Ms.toFixed(1)}ms</p>
          <p>Buffer Size: {storeStats.deltaBufferSize}</p>
          <p>Total Rows: {storeStats.totalRows}</p>
        </div>
      </div>

      {stats.endTime && (
        <div className="p-4 bg-green-100 rounded">
          <h3 className="font-semibold mb-2">Acceptance Report</h3>
          <p>Scenario: {selectedScenario?.name}</p>
          <p>Duration: {elapsed.toFixed(1)}s</p>
          <p>Deltas: {stats.deltasInjected}</p>
          <p>Avg FPS: {avgFPS.toFixed(1)}</p>
          <p>Min FPS: {minFPS.toFixed(1)}</p>
          <p>Apply p95: {storeStats.applyDurationP95Ms.toFixed(1)}ms</p>
          <p>Render p95: {storeStats.renderDurationP95Ms.toFixed(1)}ms</p>
          <p className="mt-2">
            Status: {
              avgFPS >= 55 && minFPS >= 50 && storeStats.applyDurationP95Ms < 60 && storeStats.renderDurationP95Ms < 12
                ? '✅ PASS'
                : '❌ FAIL'
            }
          </p>
        </div>
      )}
    </div>
  );
}




