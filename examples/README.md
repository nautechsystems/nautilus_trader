# Examples

Python scripts are organized by system environment context:

- **Backtest**: Historical data with simulated venues.
- **Live**: `LiveNode` examples for live, sandbox, and testnet venues.
- **Other**: Various examples beyond strategies.

Scripts within each environment context directory are organized by integration.

## Live adapter examples

Maintained Rust-native adapter testers use the generic `data_tester.py` and `exec_tester.py` names
under `live/<adapter>/`. These replace the removed adapter-prefixed tester variants.
The maintained sandbox execution tester is `live/sandbox/exec_tester.py`.

All tracked Python examples import the current public package surface. Adapter examples use
`nautilus_trader.live.LiveNode`, built-in tester configs from `nautilus_trader.testkit`, or
self-contained actors and strategies maintained with the example.

Ensure that the `nautilus_trader` package is either compiled from source or installed via pip before
running the examples. See the [installation guide](https://nautilustrader.io/docs/latest/getting_started/installation)
for more information.

From the repository root, run a Rust-native adapter tester against the venue testnet:

```bash
uv run --project python --no-sync python examples/live/lighter/data_tester.py
```

The script connects immediately and streams market data; stop it with Ctrl+C.
