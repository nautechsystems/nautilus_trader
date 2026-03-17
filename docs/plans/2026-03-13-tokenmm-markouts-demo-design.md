# TokenMM Markouts Demo Notebook Design

## Goal

Build a simple demo notebook for Jeff that answers whether current live TokenMM / MakerV3 fills are getting paid, how 30s/60s/120s FV markouts behave, how results split by side and deployment slice, and whether the current live book looks directionally up or down.

## Scope

- New notebook only: `research/tokenmm/notebooks/tokenmm_markouts_edge_pnl_demo.ipynb`
- New helper module only: `research/tokenmm/telemetry_helpers.py`
- New optional Redis FV freeze script only: `ops/scripts/export_tokenmm_markout_inputs.py`
- No dashboard, API, warehouse, schema, or production reporting changes

## Sources Of Truth

- Durable core: local SQLite telemetry under `/var/lib/nautilus/telemetry/tokenmm`
  - `execution_fill`
  - `execution_markout`
  - `order_action`
  - `quote_cycle`
  - `flux_balance_snapshot_row`
  - `portfolio_inventory_snapshot`
- Optional overlay: frozen FV stream extract built from retained Redis `flux:v1:fv:stream:{strategy_id}` rows

## Design Decisions

### Core notebook path

The notebook will always work from local SQLite only. `execution_fill` is the canonical fill dataset, and `execution_markout` is the durable markout source. The join identity is `trader_id + event_id`, with `trade_id` shown only as display context.

### Optional benchmark overlay

Fill-time edge, maker-mid comparison, and 5m/30m/1h markouts are optional sections that appear only when a frozen FV extract is present. The notebook will not hit live Redis directly during demo use. If the extract is absent, the notebook will show a short omission note and continue.

### Local market definition

For this demo only, "local market" means `maker_mid` from the FV stream payload. The notebook will state that this is an inference because historical local BBO is not durably persisted elsewhere.

### Directional PnL context

The PnL section will use the latest `flux_balance_snapshot_row` rows plus the latest `portfolio_inventory_snapshot` row to show current marked context, current inventory, average open price, realized PnL when present, and freshness/completeness caveats. It will not claim to be a true daily or net ledger.

## Notebook Sections

1. Scope and methodology
2. Data loading
3. Sanity checks
4. Recent fill sample
5. FV markouts by horizon from SQLite
6. Side split
7. By strategy / venue / symbol pivots
8. Directional PnL context
9. Takeaways / caveats
10. Optional fill-time edge and extended-horizon sections if a frozen FV extract exists

## Validation

- Unit-test helper functions
- Validate notebook JSON by parsing it with Python
- Execute the notebook source end-to-end on the local SQLite snapshot without Redis
- Spot-check a few fill-to-markout joins against raw SQLite rows
