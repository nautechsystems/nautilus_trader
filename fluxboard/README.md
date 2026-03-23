<!-- DOCID: fluxboard/readme@v1 -->

# Fluxboard

## Purpose

Describe Fluxboard’s architecture, APIs, and operational invariants so frontend and backend engineers stay aligned.

Modern React+TypeScript implementation of Fluxboard panels (Params, Trades, Market Data, FVs, PnL, Alerts).

## Scope

- Fluxboard React+TypeScript frontend application
- Local development workflow (install, dev server, build)
- Testing (Vitest, Playwright) and key panels/features

## Overview

- **Tech Stack:** React 18, TypeScript, Vite, Tailwind CSS, Zustand, Socket.IO
- **Backend:** FluxAPI (Flask). For TokenMM, default runner target is `127.0.0.1:5022`.
- **Frontend:** Vite dev server (default `http://127.0.0.1:5173`) or embedded static serving from FluxAPI at `/tokenmm/*` and `/equities/*`.
- **Features:** Real-time updates via Socket.IO (polling transport by default), session persistence, formatting parity

## Quick Start

### Prerequisites

- Node.js >= 18
- pnpm installed (`npm install -g pnpm`)
- FluxAPI running. For serving and smoke checks, start with `apps/fluxboard/docs/tokenmm_runbook.md`.
- Equities hosting uses the same Fluxboard bundle via `systems/flux/flux/runners/equities/run_api.py`.

### Installation

```bash
# from repository root
pnpm --dir fluxboard install --frozen-lockfile
pnpm --dir fluxboard exec playwright install chromium
```

### Development

For TokenMM, follow:

1. `apps/fluxboard/docs/tokenmm_runbook.md` (runner order, serving modes, and smoke checks)
2. `systems/flux/flux/runners/tokenmm/run_api.py` (serves Fluxboard at `/tokenmm/*` and Pulse at `/pulse/*`)

For equities, use `systems/flux/flux/runners/equities/run_api.py`, which serves the same Fluxboard app at `/equities/*` and can also serve Pulse at `/pulse/*`.

### Build

```bash
pnpm --dir fluxboard build
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
- **Quarantined suites**: Trades, most Params component/integration suites, Scanners, Alerts, PnL integrations (`pnpm test:full`)
- **Stable Params regression**: `pnpm test:run __tests__/ParamsBulkApplyAllRow.test.tsx`
- **E2E tests**: `pnl.spec.ts`, `params.spec.ts`, `alerts.spec.ts`, `sound.spec.ts`, `dashboard.spec.ts`, `smoke.spec.ts`

## Project Structure

Fluxboard is a Vite + React app kept under `fluxboard/`.

- **Entry/router:** `main.tsx`
- **App shell:** `App.tsx`, `Nav.tsx`, `Title.tsx`
- **Pages/surfaces:** top-level `*.tsx` pages (Dashboard/Signal/Params/Balances/Trades/Alerts/etc.)
- **Shared code:** `components/`, `config/`, `hooks/`, `lib/`, `stores/`, `utils/`
- **Tests:** `__tests__/`, `tests/`, `e2e/` (+ `*.test.tsx`)
- **Docs:** `docs/` (UI standards + TokenMM contracts/runbook)

## Features

### Params

- Strategy selector
- Live parameter editor
- Bulk and per-row saves through FluxAPI with inline validation and error toasts

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

See `fluxboard/docs/ui-standards.md` for the tokenized styling primitives (colors, spacing, typography).
New UI work should prefer tokens/theme variables over raw Tailwind color/spacing utilities.

## Socket Configuration

```ts
{
  path: '/socket.io',
  transports: ['polling'],
  reconnection: true,
  reconnectionDelay: 500,
  reconnectionDelayMax: 5000
}
```

`useWebSocket(event, handler)` remains the default legacy subscription path and makes no
standard-payload assumptions. Surfaces opting into realtime standardization can pass a
third argument with `surface`, an injected legacy `subscribe`, and a shared `bridge`
that resolves `legacy` versus `standard` mode and owns the compatibility subscription.
That keeps flag-off behavior unchanged while giving flag-on surfaces one reusable bridge
seam instead of per-panel socket glue.

## API Endpoints (Proxied to :5022)

- `GET /api/v1/signals` - Strategy state, quote status, and top-level operator signal rows
- `GET /api/v1/balances` - Balance and risk rows
- `GET /api/v1/trades?limit=<n>&offset=<n>` - Paged trades (returns totals and optional cursor fields)
- `GET /api/v1/trades?limit=<n>&cursor=<token>` - Fetch the next historical slice using the opaque cursor token
- `GET /api/v1/trades/delta` - Incremental trades refresh for live views
- `GET /api/v1/strategies` - Strategy list
- `GET /api/v1/strategies/<id>/parameters` - Strategy params
- `PATCH /api/v1/strategies/<id>/parameters` - Save params
- `GET /api/v1/param-schema` - Schema (types, bounds, defaults)
- `GET /api/v1/params` - Bulk fetch (Fluxboard initial/refresh load)
- `PATCH /api/v1/params` - Bulk save (Save All / Save Selected)
- `GET /api/v1/alerts` - Active alerts feed
- `DELETE /api/v1/alerts` - Clear alerts

## Breaking Changes

### API Migration to `/api/v1/*`

Fluxboard now consumes only the versioned FluxAPI surface; `/api/v1/*` endpoints must be used instead.

| Legacy Endpoint | Replacement |
| --- | --- |
| `/api/signals` | `/api/v1/signals` |
| `/api/balances` | `/api/v1/balances` |
| `/api/trades` | `/api/v1/trades` |
| `/api/trades/delta` | `/api/v1/trades/delta` |
| `/api/strategies` | `/api/v1/strategies` |
| `/api/strategies/<id>/parameters` (GET/POST) | `/api/v1/strategies/<id>/parameters` (GET/PATCH) |
| `/api/params` | `/api/v1/params` |
| `/api/alerts` | `/api/v1/alerts` |

**Migration guidance:** update API clients to prepend `/api/v1`, bump cached route lists, and re-run integration tests before deploying.

Fluxboard consumes the versioned `/api/v1/*` surface only. Production Flux does not include a runtime legacy `/api/*` compatibility mode.

### Fluxboard Params UX notes

- Auto-refresh pauses when editing or when unsaved changes exist. Header shows a pause reason tag.
- Save All saves only dirty cells across all strategies. Save Selected saves only the selected dirty strategies.
- Row Save appears per-strategy and is disabled until there are changes and no validation errors.
- Validation uses the server-provided schema. On failed save, the first invalid cell is focused automatically for quick correction.
- Schema-defined params not present in strategies.ini (e.g., `max_age_ms`, `freshness_mode`) are supported end-to-end.

## Deduplication

- **Trades:** By `row_id` with the highest `version` winning (idempotent upserts)
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
| Vite proxies `/api` and `/socket.io` to FluxAPI (TokenMM default: :5022) | ✅ |
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
    pnpm --dir fluxboard install --frozen-lockfile
    pnpm --dir fluxboard test:run
```

### E2E Tests (Playwright)

Requires FluxAPI reachable (TokenMM default: `127.0.0.1:5022`) and explicit opt-in:

```yaml
- name: Run Frontend E2E Tests
  run: |
    pnpm --dir fluxboard install --frozen-lockfile
    pnpm --dir fluxboard exec playwright install chromium
    FLUXBOARD_E2E=1 pnpm --dir fluxboard test:e2e
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

### Port in use

```bash
lsof -ti:5022
```

### Backend not running

```bash
# start your backend process in another terminal (from repository root)
curl -fsS http://127.0.0.1:5022/api/v1/healthz
```

### Socket not connecting

Check FluxAPI is running and serving `/socket.io` (TokenMM default: `http://127.0.0.1:5022/socket.io/...`).

### Dependencies not installing

```bash
rm -rf fluxboard/node_modules
pnpm --dir fluxboard install --frozen-lockfile
```

## References

- Core architecture: `docs/concepts/architecture.md`
- Engine/FluxAPI backend: `systems/flux/flux/` and `systems/flux/docs/api.md`
- Fluxboard UI standards: `fluxboard/docs/ui-standards.md`
- Zustand selector usage: `fluxboard/docs/SELECTORS_GUIDE.md`

## Changelog

- 2025-11-20: Updated README title/Scope and aligned references with Fluxboard docs.

## License

Internal project.
