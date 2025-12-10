# Lighter

Lighter is a decentralized perpetual futures exchange offering high-performance on-chain trading.
This integration supports live market data feeds and order execution on Lighter.

:::warning
The Lighter integration is under active development. Some features may be incomplete.
:::

## Overview

This adapter is implemented in Rust with Python bindings. It provides direct integration
with Lighter's REST and WebSocket APIs without requiring external client libraries.

The Lighter adapter includes multiple components:

- `LighterHttpClient`: Low-level HTTP API connectivity.
- `LighterWebSocketClient`: Low-level WebSocket API connectivity.
- `LighterInstrumentProvider`: Instrument parsing and loading functionality.
- `LighterDataClient`: Market data feed manager.
- `LighterExecutionClient`: Account management and trade execution gateway.

:::note
Most users will define a configuration for a live trading node (as shown below)
and won't need to work directly with these lower-level components.
:::

## Development Setup

### Building the PyO3 Extension

The Lighter adapter includes Rust components with Python bindings via PyO3. When developing
or modifying the adapter, you must rebuild the extension for changes to take effect.

:::warning
**Always use `make build-debug` for development builds.** Do not use `maturin develop` directly.
:::

The project's build system (`build.py`) ensures proper feature flags are passed to Cargo:

```bash
# Correct way to build (uses build.py with proper feature flags)
make build-debug

# This ensures features like cython-compat, ffi, python, extension-module are enabled
```

Using `maturin develop` directly may result in missing submodules or incorrect module naming
because it doesn't pass the required feature flags (`cython-compat,ffi,python,extension-module,postgres`).

### Importing the PyO3 Client

The Rust HTTP and WebSocket clients are exposed via PyO3 bindings:

```python
# Correct import path
from nautilus_trader.core.nautilus_pyo3 import lighter

# Access the clients
client = lighter.LighterHttpClient(is_testnet=True)
ws_client = lighter.LighterWebSocketClient(is_testnet=True)
```

:::note
There is no top-level `nautilus_pyo3` package. Always import via `nautilus_trader.core.nautilus_pyo3`.
:::

### Verifying the Build

After building, verify the Lighter module is properly included:

```python
from nautilus_trader.core import nautilus_pyo3

# Check lighter is available
assert hasattr(nautilus_pyo3, 'lighter')
assert hasattr(nautilus_pyo3, 'LighterHttpClient')

# List available exports
print(dir(nautilus_pyo3.lighter))
# ['LighterHttpClient', 'LighterWebSocketClient', 'get_lighter_http_base_url', 'get_lighter_ws_url']
```

If `lighter` is not available, rebuild with `make build-debug`.

## Examples

You can find live example scripts [here](https://github.com/nautechsystems/nautilus_trader/tree/develop/examples/live/lighter/).

## Testnet Setup

Lighter provides a testnet environment for testing strategies without risking real funds.

### Endpoints

- **Testnet**: `https://testnet.zklighter.elliot.ai`
- **Mainnet**: `https://mainnet.zklighter.elliot.ai`

## Symbology

Lighter uses a specific symbol format for instruments:

### Perpetual Futures

Format: `{Symbol}-USD-PERP`

Examples:

- `BTC-USD-PERP` - Bitcoin perpetual futures
- `ETH-USD-PERP` - Ethereum perpetual futures

To subscribe in your strategy:

```python
InstrumentId.from_str("BTC-USD-PERP.LIGHTER")
InstrumentId.from_str("ETH-USD-PERP.LIGHTER")
```

## Troubleshooting

### Module Import Errors

**Symptom:** `ModuleNotFoundError: No module named 'nautilus_pyo3'` or `AttributeError: module has no attribute 'lighter'`

**Cause:** Either the extension wasn't built, or it was built without the Lighter module.

**Solution:**

1. Ensure you're using the project's virtual environment: `source .venv/bin/activate`
2. Rebuild with: `make build-debug`
3. Verify with: `nm -gU nautilus_trader/core/nautilus_pyo3.*.so | grep lighter`

### Stale Build

**Symptom:** Changes to Rust code don't appear in Python.

**Solution:** Run `make build-debug` after any changes to Rust or Cython code.

### Wrong Python Environment

**Symptom:** Import works in one terminal but not another.

**Solution:** Always use `uv run python` or activate the `.venv` before running scripts:

```bash
# Option 1: Use uv
uv run python your_script.py

# Option 2: Activate venv
source .venv/bin/activate
python your_script.py
```
