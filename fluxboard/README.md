<!-- DOCID: fluxboard/readme@v1 -->

# Fluxboard

## Purpose
Describe Fluxboard’s architecture, APIs, and operational invariants so frontend and backend engineers stay aligned.

Modern React+TypeScript migration of Chainsaw GUI pages (Params, Trades, Market Data, FVs, PnL).

## Scope

- Fluxboard React+TypeScript frontend application
- Local development workflow (install, dev server, build)
- Testing (Vitest, Playwright) and key panels/features

## Overview

- **Tech Stack:** React 18, TypeScript, Vite, Tailwind CSS, Zustand, Socket.IO
- **Backend:** Flask on :5000
- **Frontend:** Served by Flask on :5000 (production) or Vite dev server (development)
- **Features:** Real-time updates via WebSocket, session persistence, exact formatting parity

## Quick Start

### Prerequisites

- Node.js >= 18
- pnpm installed (`npm install -g pnpm`)
- Flask backend running on :5000

### Installation

```bash
# from repository root
cd fluxboard
pnpm install
pnpm exec playwright install chromium
```

### Development

**Production:** Build static assets to be served by your backend:
```bash
# from repository root
cd fluxboard
pnpm build
```

Access at: http://localhost:5000

**Development:** Start Vite dev server (requires backend running on :5000 in another terminal):
```bash
# from repository root
cd fluxboard
pnpm dev
```

Ensure your backend is running on :5000 before opening the app.

### Build

```bash
pnpm build
```

Outputs to `dist/` directory.

### Testing

Run unit tests with Vitest:
```bash
pnpm test                 # Watch mode (stable subset)
pnpm test:run             # Run once (stable subset)
pnpm test:run --coverage  # With coverage
pnpm test:full            # Attempt entire suite (requires backend + sockets)
```

> ℹ️ Stable tests exclude legacy integration suites that require live sockets,
> Redis, or DOM APIs unavailable in jsdom. Set `VITEST_FULL=1` (or run
> `pnpm test:full`) once those dependencies are available.

Run E2E tests with Playwright:
```bash
# Skips unless FLUXBOARD_E2E=1
FLUXBOARD_E2E=1 pnpm test:e2e
FLUXBOARD_E2E=1 pnpm test:e2e --headed
FLUXBOARD_E2E=1 pnpm test:e2e --debug
```

Available test suites:
- **Unit tests (stable)**: Layouts, shared components, hooks, utilities
- **Quarantined suites**: Trades, Params, Scanners, Alerts, PnL integrations (`pnpm test:full`)
- **E2E tests**: `pnl.spec.ts`, `params.spec.ts`, `alerts.spec.ts`, `sound.spec.ts`, `dashboard.spec.ts`, `smoke.spec.ts`

## Project Structure

Fluxboard uses a mostly flat top-level layout with supporting folders:

- **Config:** `package.json`, `vite.config.ts`, `tsconfig.json`, `tailwind.config.ts`
- **Types:** `types.ts` - API schemas
- **Utils:** `utils.ts` - Deduplication, formatting
- **Infrastructure:** `api.ts`, `sockets.ts`, `stores.ts`
- **Components:** `Nav.tsx`, `Table.tsx`
- **Routes:** `Params.tsx`, `Trades.tsx`, `MarketData.tsx`, `FV.tsx`
- **App:** `main.tsx`, `App.tsx`
- **Tests:** `smoke.spec.ts`, `test-helpers.ts`

## Features

### Params
- Strategy selector
- Live parameter editor
- POST to Redis with error toasts

### Trades
- Server-side pagination with `< Prev` and `Next >` controls (default 100 rows)
- Page indicator: `Page X of Y`
- Page size persists in `sessionStorage`
- Active filters persist in `sessionStorage` and restore on refresh
- Live socket appends when viewing latest page (page 1, scrolled to top) with dedupe by `row_id`
- `Jump to latest` appears when viewing history (unread counter increments)
- Side coloring: buy=green, sell=red
- Row cap: 5,000

### Market Data
- Real-time price updates
- Socket merge by `(exchange, symbol)` key
- Latency formatted to 1 decimal
- Row cap: 2,000

### Signal
- Strategy grid mirrors FluxAPI `/api/v1/signals`
- `Bal` column shows readiness badges (OK/WARN/FAIL/UNKNOWN) with tooltips listing missing tokens
- Summary strip aggregates badge counts so operators can scan inventory status
- Badges automatically hide when backend sets `BALANCE_READINESS_ENABLED=0`.

### FVs
- Dynamic columns from API
- Console warnings for unexpected keys
- Row cap: 2,000

### PnL
- Main operator view for live profitability
- Time windows: 15m, 1h, 4h, 24h, Last N, All (all minutes-based presets are evaluated relative to *current* UTC time; if there are no fills in that window, the report is empty even if older blotter entries exist)
- Base filter (canonical tokens) and advanced fee controls
- Summary cards: Gross/Net PnL in bps/USD, hedge ratio and coverage
- Groups table: grouped by signal_id with VWAPs and PnL
- By-symbol table: FV(now) integration with source badges (snapshot/strategy/md), mark-to-market ($), flow, coverage, and risk flags (loss, stale FV)
- Unhedged positions: FV-marked VaR-lite estimate (3%) with small-position toggle
- Optional decision details: when `VITE_TRADES_DECISION_DETAILS` (localStorage key `fluxboard:feature:trades-decision-details`) is enabled, Trades/PnL views show `decision_summary` and “Decision vs Realized” columns derived from the `decision_log_v1` schema; this flag is disabled by default.

## Formatting Invariants

### Column Order
Matches legacy exactly:
- **Trades:** time | exchange | coin | side | price | qty | notional | fee | notes
- **Market Data:** exchange | symbol | bid | ask | latency_ms | publisher | timestamp

### Number Formats
- Prices/qty: rendered as strings (no coercion)
- `latency_ms`: 1 decimal (`toFixed(1)`)
- Timestamps: rendered exactly as delivered by API

### Visual
- Zebra striping: `odd:bg-neutral-900`
- Side colors: buy=`text-emerald-400`, sell=`text-red-400`
- Compact density: `text-xs`, `p-2` padding

## Socket Configuration

```ts
{
  path: '/socket.io',
  transports: ['websocket', 'polling'],
  reconnection: true,
  reconnectionDelay: 500,
  reconnectionDelayMax: 5000
}
```

## API Endpoints (Proxied to :5000)

- `GET /api/v1/market-data/snapshot` - Market snapshots
- `GET /api/v1/trades?limit=<n>&offset=<n>` - Paged trades (returns totals and optional cursor fields)
- `GET /api/v1/trades?limit=<n>&cursor=<token>` - Fetch the next historical slice using the opaque cursor token
- `GET /api/v1/fvs` - Fair values
- `GET /api/v1/strategies` - Strategy list
- `GET /api/v1/strategies/<id>/parameters` - Strategy params
- `PATCH /api/v1/strategies/<id>/parameters` - Save params
- `GET /api/v1/param-schema` - Schema (types, bounds, defaults)
- `GET /api/v1/params` - Bulk fetch (Fluxboard initial/refresh load)
- `PATCH /api/v1/params` - Bulk save (Save All / Save Selected)
- `GET /export_blotter` - Export trades (future)

## Breaking Changes

### API Migration to `/api/v1/*`

Fluxboard now consumes only the versioned FluxAPI surface; `/api/v1/*` endpoints must be used instead.

| Legacy Endpoint | Replacement |
| --- | --- |
| `/api/market_data` | `/api/v1/market-data/snapshot` |
| `/api/trades` | `/api/v1/trades` |
| `/api/trades/delta` | `/api/v1/trades/delta` |
| `/api/fvs` | `/api/v1/fvs` |
| `/api/strategies` | `/api/v1/strategies` |
| `/api/strategies/<id>/parameters` (GET/POST) | `/api/v1/strategies/<id>/parameters` (GET/PATCH) |
| `/api/params` | `/api/v1/params` |
| `/api/pnl` and `/api/pnl/csv` | `/api/v1/pnl` and `/api/v1/pnl/csv` |

**Migration guidance:** update API clients to prepend `/api/v1`, bump cached route lists, and re-run integration tests before deploying. Monitor `fluxapi_legacy_requests_total` to ensure no callers still hit `/api/*`.

**FluxAPI compat boundary flag:** `fluxapi.web` includes legacy `/api/*` compatibility routes by default (`FLUXAPI_ENABLE_LEGACY_COMPAT=1`) to preserve existing behavior. Set `FLUXAPI_ENABLE_LEGACY_COMPAT=0` to exclude legacy compat routes at startup.

### Fluxboard Params UX notes

- Auto-refresh pauses when editing or when unsaved changes exist. Header shows a pause reason tag.
- Save All saves only dirty cells across all strategies. Save Selected saves only the selected dirty strategies.
- Row Save appears per-strategy and is disabled until there are changes and no validation errors.
- Validation uses the server-provided schema. On failed save, the first invalid cell is focused automatically for quick correction.
- Schema-defined params not present in strategies.ini (e.g., `max_age_ms`, `freshness_mode`) are supported end-to-end.

## Deduplication

- **Trades:** By `trade_id` (warn and skip duplicates)
- **Market Data:** By `(exchange, symbol)` tuple (merge updates)

## Error Handling

- POST failures show toast with HTTP status
- Unknown FV columns render without crash (console.warn)
- Socket reconnection automatic

## Acceptance Checklist

| Check | Status |
| --- | --- |
| Fluxboard lives under `fluxboard/` in this repository | ✅ |
| Commands run from repo root with `cd fluxboard` | ✅ |
| All files in single `fluxboard/` directory | ✅ |
| Vite proxies `/api`, `/socket.io`, `/export_blotter` to :5000 | ✅ |
| Timestamps render exactly as delivered | ✅ |
| Trades pagination uses `sessionStorage` | ✅ |
| Socket deduplication working | ✅ |
| POST failures show toast | ✅ |
| Row caps enforced | ✅ |
| Params stored as `Record[str, str]` | ✅ |
| Column order matches legacy | ✅ |
| Side colors: buy=green, sell=red | ✅ |
| Zebra striping on tables | ✅ |

## CI/CD Integration

To integrate frontend tests into the existing CI pipeline:

### Unit Tests (Vitest)
Add to your CI workflow:
```yaml
- name: Run Frontend Unit Tests
  run: |
    cd fluxboard
    pnpm install
    pnpm test:run
```

### E2E Tests (Playwright)
Requires Flask backend running on :5000:
```yaml
- name: Run Frontend E2E Tests
  run: |
    cd fluxboard
    pnpm install
    pnpm exec playwright install chromium
    pnpm test:e2e
```

### Test Coverage
- Unit tests: >80% coverage target
- E2E tests: Smoke coverage for critical user flows
- Performance: No regressions in component render times

## Known Gaps

- Theme switching not implemented (v0.1 out of scope)
- Latency page and dashboard panel builder deferred
- Catalog moved to separate `nexus` app

## Development Notes

- TypeScript strict mode enabled
- All params treated as strings (no client coercion)
- Socket events match legacy exactly
- Vite HMR for fast development

## Troubleshooting

### Port 5000 in use
```bash
lsof -ti:5000 | xargs kill -9
```

### Backend not running
```bash
# start your backend process in another terminal (from repository root)
curl -fsS http://localhost:5000/api/v1/healthz
```

### Socket not connecting
Check Flask is running on :5000 and serving `/socket.io` endpoint.

### Dependencies not installing
```bash
rm -rf node_modules pnpm-lock.yaml
pnpm install
```

## References

- Core architecture: `docs/concepts/architecture.md`
- Engine/FluxAPI backend: `nautilus_trader/flux/` and `docs/flux/api.md`
- Fluxboard UI standards: `fluxboard/docs/ui-standards.md`
- Zustand selector usage: `fluxboard/docs/SELECTORS_GUIDE.md`

## Changelog

- 2025-11-20: Updated README title/Scope and aligned references with Fluxboard docs.

## License

Internal project - see main Chainsaw repo.
