# Rithmic Notebook Sandbox

These notebooks are for local validation of the Rithmic shadow-port branch in a dedicated sandbox clone.

They are not intended to run a production live `TradingNode` inside Jupyter. Use them for short smoke checks and exploratory validation only.

## Intended workflow

From the sandbox repo root:

```bash
uv sync --all-groups --all-extras --no-install-project
PYO3_PYTHON=$(pwd)/.venv/bin/python BUILD_MODE=debug-pyo3 .venv/bin/python build.py
uv pip install --python .venv/bin/python jupyterlab ipykernel
.venv/bin/python -m ipykernel install --user --name nautilus-rithmic-lab --display-name "Python (nautilus-rithmic-lab)"
.venv/bin/jupyter lab
```

Open:

- `examples/live/rithmic/notebooks/rithmic_adapter_sandbox_smoke.ipynb`

## Environment

Load your Rithmic credentials into the shell before starting Jupyter so the notebook kernel inherits them.

Minimum data-side variables:

- `RITHMIC_USERNAME`
- `RITHMIC_PASSWORD`
- `RITHMIC_SYSTEM_NAME`

Execution-path cells also require:

- `RITHMIC_ACCOUNT_ID`

Profile-scoped variables also work through `RITHMIC_PROFILE` plus matching `RITHMIC_{PROFILE}_*` names.

`RITHMIC_PROFILE` is only a local namespace for those environment variables. The actual Rithmic
connection values are still `SYSTEM_NAME`, `FCM_ID`, and `IB_ID`.

Do not guess those broker-facing values from the firm name alone. Some connections use
`paper_trading`, Apex uses `Apex`, and other Rithmic brokers can use non-obvious identifiers on
both prop-firm accounts and standard demo/live accounts. The source of truth is the RTrader Pro
desktop application under `File > User Profile`, where you should copy `System`, `FCM`, and `IB`
exactly as shown.

## Safety

- The market-data and bars cells are enabled by default.
- The order, bracket, and OCO cells are disabled by default.
- Enable order-path cells only on demo credentials unless you intentionally want to exercise live routing.

Control flags:

- `RITHMIC_SANDBOX_RUN_MARKET_DATA=1`
- `RITHMIC_SANDBOX_RUN_BARS=1`
- `RITHMIC_SANDBOX_RUN_ORDER=0`
- `RITHMIC_SANDBOX_RUN_BRACKET=0`
- `RITHMIC_SANDBOX_RUN_OCO=0`

## Notes

The notebook uses subprocess-driven smoke steps where possible so the actual adapter code runs under the sandbox `.venv` without depending on Jupyter's event loop for long-lived live-node behavior.

The current passing smoke path is:

- import/config sanity through the local built package
- live quote subscription through the low-level bindings provider plus `RithmicGateway.from_env(...)`
- historical bars through an explicit history-enabled `RithmicGateway(...)`

That explicit history gateway matters because `RithmicGateway.from_env(...)` leaves the history plant disabled by default.

Additional adapter caveats:

- historical API usage is plan-limited; on basic Rithmic plans, historical downloads are typically capped at `20 GB` per month
- Rithmic sends warning emails to the registered account email address when API usage approaches the limit or when their access rules are being breached
- ignoring those warning emails can lead to automatically triggered temporary restrictions, so monitor the registered inbox during large backfills
- live external bar subscriptions can use history-plant time bars or tick bars
- historical tick-bar replay is currently limited to `1-TICK`; the adapter will reject larger historical tick-bar requests rather than re-aggregate locally
- native historical `N-TICK` replay is expected to land later via upstream `rithmic-rs` support, at which point the adapter should pass it through unchanged
- history replies can be truncated venue-side; if you see a round-number result such as `10000` bars, or the returned range is shorter than requested, retry with smaller windows because the adapter does not auto-resume via `request_key` yet
