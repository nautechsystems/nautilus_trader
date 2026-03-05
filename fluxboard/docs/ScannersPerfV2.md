<!-- DOCID: ScannersPerfV2.md@v1 -->
Last updated: 2025-12-10 · commit 70da26dc

<!-- DOCID: fluxboard/scanners-perf-v2@v1 -->

# Scanners Perf V2

## Purpose

Describe the Scanners Perf V2 path so engineers can enable, validate, and, if necessary, roll back the optimized ScannersTable implementation.

## Scope

- Frontend ScannersTable performance pipeline (store, component, feature flags)
- Backend stats ingestion endpoint and exporter
- Perf harness and acceptance criteria for enabling the feature in production

## Interface

- Frontend flag helper: `isScannersPerfV2Enabled()`
- Env/local feature flags:
  - `VITE_SCANNERS_PERF_V2`
  - `fluxboard:feature:scanners-perf-v2`
- Backend API: `POST /api/v1/scanners/perf-stats`
- Redis key: `fluxboard:scanners:perf_v2:stats`
- Prometheus exporter: `fluxboard_perf_exporter.py` (port `9092`)

## Prereqs

- Fluxboard dev environment running with Scanners panel enabled
- FluxAPI backend available on `:5000`
- Optional: Prometheus + Grafana stack configured for `fluxboard_perf_exporter.py`

## Procedure

1. Enable `Scanners Perf V2` via env or localStorage.
2. Use the perf harness to drive synthetic scenarios (5k/10k rows @ 100–300 Hz).
3. Observe metrics locally (browser dev tools + exporter) and compare against acceptance criteria.
4. When satisfied, promote to staging and then production behind the feature flag.

## Validation

- Follow the **Acceptance Criteria** section below for latency, FPS, CPU, and GC budgets.
- Use the **Performance Harness** section to reproduce high-throughput load.
- Confirm Grafana panels and Prometheus metrics match expectations.

## Rollback

- Disable `Scanners Perf V2` by flipping `VITE_SCANNERS_PERF_V2=0` or clearing the localStorage flag.
- Redeploy Fluxboard without changing backend code; all code paths are gated by `isScannersPerfV2Enabled()`.
- Use the **Rollback Plan** checklist below during staged or production rollbacks.

## Troubleshooting

See the **Troubleshooting** section at the end of this document for common issues and checks (buffer size, slow apply times, render jank, and missing metrics).

## FAQ

- **Q:** Is Perf V2 safe to enable without the exporter?
  **A:** Yes. Metrics publishing is additive; missing exporter only affects observability.
- **Q:** Do I need virtualization to use Perf V2?
  **A:** No. Perf V2 works with and without `scannersVirtualizedV1` enabled.

## Examples

- Example scenarios: `tools/perf/scannersHarness/scenarios/*.json`
- Example harness entrypoint: `fluxboard/pages/ScannersHarness.tsx`

## References

- Backend blueprint: `fluxapi/blueprints/scanners.py`
- Exporter: `scripts/exporters/fluxboard_perf_exporter.py`
- Architecture doc: `docs/architecture/scanners-performance-improvements.md`

## Changelog

- 2025-11-20: Converted to standard doc structure; content condensed and cross-references added.

---

## Architecture

### Store Layer (`fluxboard/stores/scannersStore.ts`)

**State:**

- `rowsById`: Map<pool_address, ScannerPricingSnapshot> - Raw snapshots
- `enrichedById`: Map<pool_address, EnrichedRow> - Enriched with precomputed display strings
- `sortedIdsByEdge`: string[] - Sorted by best edge DESC, last_update_ts DESC, pool_address ASC
- `filteredIds`: string[] - Filtered subset of sortedIdsByEdge
- `deltaBuffer`: Map<pool_address, ScannerPricingSnapshot> - Buffered deltas awaiting apply

**Key Functions:**

- `enqueueDelta()`: Buffer delta, schedule rAF apply, track dropped deltas in coarse mode
- `drainDeltaBuffer()`: Apply all buffered deltas, update indices incrementally
- `enrichSnapshot()`: Transform snapshot → EnrichedRow with preformatted strings (cache-aware)
- `updateSortedIndex()`: Binary insert/remove for O(log n) sort maintenance
- `publishStatsToRedis()`: Publish metrics to Redis every 1.5s (when perfV2 enabled)

**Coarse Mode:**
When delta buffer exceeds 5,000 entries, drop intermediate updates per pool (keep latest only). This prevents buffer bloat during bursts.

**Performance Marks:**

- `scanners.delta.enqueue` - When delta arrives
- `scanners.delta.apply.start/end` - Around drainDeltaBuffer
- `scanners.index.update.start/end` - Around updateSortedIndex

### Component Layer (`fluxboard/components/domain/scanners/ScannersTable.tsx`)

**Render Performance Tracking:**

- Marks `scanners.render.table.start` before render
- Measures duration in `useLayoutEffect` after DOM updates
- Records p50/p95 via `recordRenderDuration()`

**Preformatted Strings:**
Cell renderers check `isScannersPerfV2Enabled()` and use precomputed display strings when available:

- `bestEdgeDisplay`, `netEdgeSellDisplay`, `netEdgeBuyDisplay` for edge columns
- `vol24Display`, `tvlDisplay` for volume/TVL columns
- Falls back to legacy formatting when perfV2 disabled

### Backend API (`fluxapi/blueprints/scanners.py`)

**Endpoint:**

- `POST /api/v1/scanners/perf-stats` - Accepts performance stats from frontend
- Stores in Redis hash: `fluxboard:scanners:perf_v2:stats`
- Rate limited: 2 RPS, burst 5
- TTL: 5 minutes

### Grafana Exporter (`scripts/exporters/fluxboard_perf_exporter.py`)

**Metrics Exposed:**

- `fluxboard_scanners_updates_per_sec` (Gauge)
- `fluxboard_scanners_apply_duration_ms` (Histogram, buckets: [5, 10, 25, 50, 100, 200])
- `fluxboard_scanners_index_update_duration_ms` (Histogram)
- `fluxboard_scanners_render_duration_ms` (Histogram)
- `fluxboard_scanners_visible_rows` (Gauge)
- `fluxboard_scanners_total_rows` (Gauge)
- `fluxboard_scanners_dropped_delta_rate` (Gauge, 0-100)
- `fluxboard_scanners_delta_buffer_size` (Gauge)
- `fluxboard_scanners_delta_buffer_high_water` (Gauge)

**Configuration:**

- Port: 9092 (default)
- Poll interval: 5s (default)
- Redis key: `fluxboard:scanners:perf_v2:stats`

## Feature Flags

### `scanners.perfV2`

**Environment:** `VITE_SCANNERS_PERF_V2`
**LocalStorage:** `fluxboard:feature:scanners-perf-v2`
**Default:** `false` (opt-in)

**Enables:**

- rAF delta coalescing
- Incremental index updates
- Preformatted display strings
- Performance marks/measures
- Redis stats publishing
- Optimized age ticker

**Independent of:** `scannersVirtualizedV1` (can be enabled separately)

## Configuration

### Constants (in `scannersStore.ts`)

- `DELTA_QUEUE_MAX = 5_000` - Coarse mode threshold
- `THROTTLE_HIDDEN_MS = 5_000` - Age tick interval when document hidden
- `IDLE_TICK_MS = 2_000` - Age tick interval when idle
- `IDLE_DETECTION_MS = 2_000` - Idle detection window
- `REDIS_STATS_UPDATE_INTERVAL_MS = 1_500` - Stats publish interval

## Metrics & Telemetry

### Store Stats (`ScannerStats`)

- `applyDurationP50Ms`, `applyDurationP95Ms`, `applyDurationP99Ms` - Delta apply latencies
- `indexUpdateDurationP50Ms`, `indexUpdateDurationP95Ms` - Index update latencies
- `renderDurationP50Ms`, `renderDurationP95Ms` - Render latencies
- `droppedDeltas`, `droppedDeltaRatePct` - Coarse mode drop tracking
- `deltaBufferSize`, `deltaBufferHighWater` - Buffer metrics
- `updatesPerSec` - Update rate
- `virtualRowsRendered`, `totalRows` - Row counts

### Performance Marks

- `scanners.delta.enqueue` - Delta received
- `scanners.delta.apply.start/end` - Apply batch
- `scanners.index.update.start/end` - Index update (per row)
- `scanners.render.table.start/end` - Table render

## Performance Harness

### Backend Generator (`tools/perf/scannersHarness/backend/generator.py`)

Generates synthetic scanner snapshots and streams deltas.

**Usage:**

```bash
python tools/perf/scannersHarness/backend/generator.py --rows 10000 --rate 100 --duration 60 --output base.json --deltas-output deltas.json
```text

### Frontend Harness (`fluxboard/pages/ScannersHarness.tsx`)

Dev-only page at `/scanners-harness` for running performance scenarios.

**Scenarios:**
- 5k @ 100Hz: 5,000 rows, 100 updates/sec
- 10k @ 100Hz: 10,000 rows, 100 updates/sec
- 10k @ 300Hz: 10,000 rows, 300 updates/sec

**Features:**
- Inject synthetic deltas into store
- Track FPS, commit durations
- Generate acceptance report
- Validate KPIs

### Scenario Files (`tools/perf/scannersHarness/scenarios/`)

JSON files defining test scenarios:
- `5k_100hz.json`
- `10k_100hz.json`
- `10k_300hz.json`

## Acceptance Criteria

- **Smoothness:** ≥55-60 FPS while receiving 1-2k deltas/sec over 2k-10k rows
- **CPU:** <30% main-thread average during sustained bursts
- **GC:** No >50ms pauses within 30s burst
- **Latency:** enqueue→applied p50 <25ms, p95 <60ms @ 1k deltas/sec
- **Render:** table commit p95 <12ms for ~40 row window

## Rollback Plan

1. **Immediate:** Set `scanners.perfV2 = false` in localStorage or env var
2. **Code:** All Perf V2 code is gated behind `isScannersPerfV2Enabled()` check
3. **Monitoring:** Watch Grafana dashboard for regressions
4. **Gradual:** Enable in staging → canary → production

## Troubleshooting

### High Buffer Size

- Check `deltaBufferSize` in Grafana
- If consistently >5k, coarse mode should activate
- Verify rAF scheduling is working (check browser console for marks)

### Slow Apply Times

- Check `applyDurationP95Ms` in Grafana
- If >60ms, consider reducing update rate or increasing merge delay
- Verify incremental index updates are working (check `indexUpdateDurationP95Ms`)

### Render Jank

- Check `renderDurationP95Ms` in Grafana
- If >12ms, verify preformatted strings are being used
- Check React Profiler for component re-renders

### Missing Metrics

- Verify exporter is running: `curl http://localhost:9092/metrics`
- Check Redis key exists: `./scripts/ops/redis.sh HGETALL fluxboard:scanners:perf_v2:stats`
- Verify frontend is publishing: Check browser network tab for POST to `/api/v1/scanners/perf-stats`

## Testing

### Unit Tests

**Backend API (`tests/unit/fluxapi/test_scanner_pricing_api.py`):**
- `test_scanner_perf_stats_endpoint`: Verifies POST `/api/v1/scanners/perf-stats` stores metrics in Redis with correct TTL
- `test_scanner_perf_stats_rate_limiting`: Verifies rate limiting (2 req/sec, burst 5)
- `test_scanner_perf_stats_missing_fields`: Verifies graceful handling of missing fields (defaults to 0)

**Frontend Feature Flags (`fluxboard/__tests__/config/featureFlags.test.ts`):**
- Verifies `scannersPerfV2` flag defaults to `false`
- Verifies flag structure and helper functions

**Store Perf V2 (`fluxboard/__tests__/stores/scannersStorePerfV2.test.ts`):**
- Verifies preformatted strings enrichment
- Verifies delta coalescing via `enqueueDelta()`
- Verifies performance metrics tracking (`recordRenderDuration`, `recordScroll`)
- Verifies stats structure includes Perf V2 metrics

### Running Tests

```bash
# Backend API tests
pytest tests/unit/fluxapi/test_scanner_pricing_api.py -v

# Frontend tests (requires Node/Vitest)
cd fluxboard && npm test -- featureFlags.test.ts
cd fluxboard && npm test -- scannersStorePerfV2.test.ts
```bash

## Deployment

### Staging

1. Enable flag: `VITE_SCANNERS_PERF_V2=1` in staging env
2. Start exporter: `python scripts/exporters/fluxboard_perf_exporter.py`
3. Verify Prometheus scraping: Check `/targets` in Prometheus UI
4. Verify Grafana dashboard: Check metrics appear

### Production

1. Enable flag for canary users via localStorage override
2. Monitor metrics for 24-48 hours
3. Gradually enable for all users
4. Keep legacy path available for 1 release cycle

## References

- Plan: `scanners-perf-v2-implementation.plan.md`
- Store: `fluxboard/stores/scannersStore.ts`
- Component: `fluxboard/components/domain/scanners/ScannersTable.tsx`
- API: `fluxapi/blueprints/scanners.py`
- Exporter: `scripts/exporters/fluxboard_perf_exporter.py`
- Dashboard: `monitoring/grafana/dashboards/fluxboard_scanners_perf.json`
