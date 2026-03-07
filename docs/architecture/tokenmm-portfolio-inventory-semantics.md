# TokenMM Portfolio Inventory Semantics

TokenMM must have one shared portfolio source of truth.

## Ownership

- `flux.runners.tokenmm.run_portfolio` owns the canonical shared TokenMM portfolio snapshot.
- MakerV3 strategies publish per-strategy portfolio components only.
- Flux API and Fluxboard consume the shared portfolio snapshot; they must not recompute shared global risk independently.

## Shared Portfolio Snapshot

The shared snapshot must carry:

- shared inventory aggregate
- contributor diagnostics
- merged balances rows
- merged balances totals

The inventory portion must include:

- `local_qty_base` per published component
- `global_qty_base`
- `aggregation_mode`
- `global_qty_base_complete`
- `usable_component_count`
- `expected_component_count`
- `missing_required`
- `stale_required`
- `null_qty_required`

Compatibility aliases may remain temporarily:

- `local_qty` mirrors `local_qty_base`
- `global_qty` mirrors `global_qty_base`
- `global_qty_complete` mirrors `global_qty_base_complete`

## Aggregation Modes

### `strict`

- `global_qty_base` is only usable when all required contributors are fresh and known.
- missing, stale, or null required contributors force `global_qty_base = null`.

### `partial`

- `global_qty_base` is the sum of fresh known contributors.
- `global_qty_base_complete = false` when any required contributor is missing, stale, or unknown.
- missing/stale/unknown contributors remain visible in diagnostics.

## Consumer Rules

- Strategy risk may consume partial `global_qty_base` only when explicitly configured to allow it.
- Signal must show canonical strategy-local `local_qty_base` plus shared `global_qty_base` from the portfolio snapshot when present.
- `Balances(profile=tokenmm)` must use the merged balances rows from the portfolio snapshot instead of recomputing TokenMM portfolio semantics independently.
- Signals must not derive risk quantities from balances except in explicit compatibility fallback mode for older payloads.
- No consumer should infer completeness from `global_qty_base` alone.

## Normalization Alignment

This policy is orthogonal to base-unit normalization.

- the canonical fields are `local_qty_base` and `global_qty_base`
- the temporary compatibility fields are `local_qty` and `global_qty`
- the partial-vs-strict policy stays the same when the quantity source is upgraded
