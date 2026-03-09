# TokenMM Risk Source Of Truth

TokenMM risk correctness depends on one ownership model:

- each MakerV3 strategy owns only its local maker-leg truth
- `run_portfolio` owns only the shared TokenMM portfolio truth
- API and Fluxboard render those sources; they do not invent alternative risk engines
- Fluxboard balances/risk drilldown must consume backend-authored `risk_groups`, `risk_groups[].rows`, and row-level `risk_key` / `risk_label` semantics instead of locally bucketing coins.

## Canonical Quantities

- `local_qty_base` is the canonical local maker-leg base exposure seen by the strategy.
- `global_qty_base` is the canonical shared portfolio aggregate owned by `run_portfolio`.
- Compatibility aliases such as `local_qty` and `global_qty` may remain temporarily, but they must mirror the corresponding `*_base` fields exactly and be documented as aliases.
- `risk_delta` is a best-effort diagnostic proxy. It is not the canonical quantity field for local spot inventory and must not silently replace `local_qty_base`.

## Ownership Contract

### Strategy-local truth

- Spot MakerV3 strategies compute local risk from visible maker-venue account balances.
- Perp MakerV3 strategies compute local risk from fresh maker venue position truth, with reconciled cache state used only when the venue truth is explicitly confirmed safe.
- Strategy state, pricing debug, pricing adjustments, balances publisher rows, and published inventory components must all use the same local quantity source.

### Shared portfolio truth

- `run_portfolio` computes shared TokenMM inventory, contributor diagnostics, merged balances rows, and shared totals.
- `Balances(profile=tokenmm)` renders the shared portfolio snapshot only when the snapshot is fresh enough to trust: `server_ts_ms` and inventory `ts_ms` must be within `stale_after_ms`, otherwise the API falls back to the live per-strategy merge path.
- `Balances(profile=tokenmm)` must expose backend-authored `risk_groups` plus `risk_groups[].rows`, and each balance row must carry the matching `risk_key` / `risk_label` used for Fluxboard drilldown.
- `Signals(profile=tokenmm)` may render both strategy-local and portfolio-global quantities, but it must source those values from strategy state and portfolio metadata rather than deriving them from balances except in explicit compatibility fallback mode.

## Reconciliation Contract

- Missing, stale, or unreconciled truth must degrade explicitly instead of publishing fabricated zeroes.
- Startup reconciliation failure means degraded or blocked trading, not best-effort stale-cache trading.
- Runtime divergence between venue truth and cached local truth must invalidate local risk until the source is reconciled again.

## Observed Failures And Root Causes

- stale maker perp position source: strategy local risk preferred stale cache state over fresh venue position truth
- multi-account spot venue mismatch: strategy-local spot inventory did not aggregate all visible maker-venue accounts consistently
- duplicate stable cash scope collapse: balance merges could replace a non-zero scoped cash row with a newer zero row
- partial shared-global semantics confusion: shared `global_qty` was easy to misread as complete truth when only a partial aggregate was known
