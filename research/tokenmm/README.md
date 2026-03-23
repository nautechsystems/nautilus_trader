# TokenMM Research

This directory is the notebook root for the optional localhost-only `flux@tokenmm-jupyter.service`.

## Included notebooks

- `notebooks/tokenmm_trade_data.ipynb` loads local SQLite telemetry tables for exploratory live TokenMM fill, order, quote-cycle, and durable markout inspection.
- `notebooks/tokenmm_markouts_edge_pnl_demo.ipynb` is the SQLite-first demo notebook for live TokenMM / MakerV3 markouts, side splits, pivots, and directional marked context.

## Demo notebook inputs

- Both notebooks default to `TOKENMM_TELEMETRY_DIR=/var/lib/nautilus/telemetry/tokenmm`.
- The demo notebook reads:
  - `execution_fill` from `fills.sqlite`
  - `execution_markout` from `markouts.sqlite`
  - `order_action` from `orders.sqlite`
  - `quote_cycle` from `quote_cycles.sqlite`
  - optional balance / portfolio snapshot tables for directional context
- Optional fill-time edge and extended-horizon sections activate only when a frozen FV extract exists at `research/tokenmm/data/tokenmm_fv_extract.csv`.

## Quantity semantics

- Research helpers treat explicit base-quantity fill fields as canonical when they are present:
  - `last_qty_base` / `fill_qty_base` feed `fill_qty_num`
  - `last_qty_venue` / `fill_qty_venue` are preserved as secondary/debug quantity context
- Older rows without explicit normalized columns still fall back to raw `last_qty` / `fill_qty`, so pre-rollout telemetry should be interpreted with that caveat.

## Manual start

```bash
uv run --group notebook jupyter lab \
  --no-browser \
  --ip=127.0.0.1 \
  --port=8888 \
  --ServerApp.allow_remote_access=False \
  --ServerApp.root_dir="$(pwd)/research/tokenmm"
```
