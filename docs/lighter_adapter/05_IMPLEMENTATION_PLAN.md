# 05_IMPLEMENTATION_PLAN.md

## Implementation Plan

**Source**: PR-by-PR blueprint with the markdown runbook as a checklist. Architecture is Rust-first
(`crates/lighter` + PyO3 bindings) with a thin Python layer for configs/factories/tests.

### PR0 — Scaffolding
- Create `crates/lighter` workspace member with feature flags for testnet/mainnet.
- Add PyO3 bindings and Python package skeleton (`nautilus_trader/adapters/lighter/` configs,
  constants, factories).
- Define shared constants/enums, config dataclasses, and venue IDs.
- Add CI placeholders (lint/tests) and basic doc index references.
- Exit: build succeeds, configs/enums unit-tested; no network calls.

### PR1 — Instrument Discovery (public REST)
- Implement HTTP client for public `orderBooks` (and related metadata) using Rust.
- Map metadata to `CryptoPerpetual` instruments; expose via `InstrumentProvider`.
- Store recorded testnet fixtures under `tests/test_data/lighter/http/`.
- Exit: `load_all_async` returns full market list using fixtures; type/precision validated.

### Validation Spike — Signing/Auth/WS Schema
- Goal: unblock execution/private flows. Run against testnet with throwaway keys.
- Capture successful `sendTx` (or equivalent) request/response to confirm signing algorithm,
  hashing, nonce handling, and token requirements.
- Capture private REST + WS payloads to lock channel names, schemas, and snapshot/delta behavior.
- Store captures in `tests/test_data/lighter/{http,ws}/` with redactions.
- Exit: documented signing recipe, confirmed token requirement, confirmed WS schemas.

### PR2 — Public Market Data
- Build WS client for order books/trades/market stats with offset tracking and resync.
- Implement REST snapshot fetch + delta application; auto-resync on gap detection.
- Expose `LiveMarketDataClient` bindings; add fixture-backed tests for parsers/state machine.
- Exit: deterministic book sync using recorded fixtures; reconnection/resubscription logic covered.

### PR3 — Execution (gate: Validation Spike complete)
- Implement signing + nonce manager per validated recipe; support `sendTx`/`sendTxBatch`.
- Add private WS subscriptions for orders/fills and reconciliation on reconnect.
- Expose `LiveExecutionClient` bindings; add integration-style tests using captured fixtures/mocks.
- Exit: end-to-end order submit/cancel flow passes against fixtures; reconcilers handle reconnect.

### PR4 — Account, Positions, Funding
- Implement account/positions REST + WS streams; map to Nautilus portfolio events.
- Handle funding payments and fee reporting (using validated fee schedule).
- Add reconciliation routines on reconnect and periodic refresh.
- Exit: position/balance/funding reports consistent with recorded payloads.

### PR5 — Hardening & Release
- Rate-limit and backoff strategies tuned for Standard/Premium limits.
- Auth token refresh (if required) with proactive renewal and retry handling.
- Metrics/logging, documentation updates, and release notes.
- Exit: soak test/stability run completed; docs reference captured fixtures and validated behavior.
