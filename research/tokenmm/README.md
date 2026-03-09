# TokenMM Research

This directory is the notebook root for the optional localhost-only `flux@tokenmm-jupyter.service`.

## Included notebook

- `notebooks/tokenmm_trade_data.ipynb` loads local SQLite telemetry tables:
  - `execution_fill` from `fills.sqlite`
  - `order_action` from `orders.sqlite`
  - `quote_cycle` from `quote_cycles.sqlite`
- The notebook defaults to `TOKENMM_TELEMETRY_DIR=/var/lib/nautilus/telemetry/tokenmm`.
- The final markdown cell notes how to swap the SQLite reads for shipped Postgres tables later.

## Manual start

```bash
uv run --group notebook jupyter lab \
  --no-browser \
  --ip=127.0.0.1 \
  --port=8888 \
  --ServerApp.allow_remote_access=False \
  --ServerApp.root_dir="$(pwd)/research/tokenmm"
```
