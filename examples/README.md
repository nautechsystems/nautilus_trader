# Examples

Python scripts are organized by system environment context:

- **Backtest**: Historical data with simulated venues.
- **Sandbox**: Real-time data with simulated venues.
- **Live**: `LiveNode` examples for live, sandbox, and testnet venues.
- **Other**: Various examples beyond strategies.

Scripts within each environment context directory are organized by integration.

## Live adapter examples

Maintained Rust‑native adapter testers use the generic `data_tester.py` and `exec_tester.py` names
under `live/<adapter>/`. Adapter‑prefixed scripts in the same directories use the legacy API.

Examples that import split v1‑only modules such as `nautilus_trader.live.node`,
`nautilus_trader.examples`, or `nautilus_trader.test_kit`, or that use migration‑only names such as
`LoggingConfig` and `TradingNodeConfig`, remain as legacy references. This includes the scripts under
the top‑level `sandbox/` directory; the current sandbox execution tester is
`live/sandbox/exec_tester.py`. Current adapter testers use `nautilus_trader.live.LiveNode` and the
built‑in tester configs from `nautilus_trader.testkit`.

Legacy scripts and notebooks remain when they demonstrate venue‑specific subscriptions, strategy
configuration, or order behavior that a current tester does not preserve.

Ensure that the `nautilus_trader` package is either compiled from source or installed via pip before
running the examples. See the [installation guide](https://nautilustrader.io/docs/latest/getting_started/installation)
for more information.

From the repository root, run a Rust‑native adapter tester against the venue testnet:

```bash
.venv/bin/python examples/live/lighter/data_tester.py
```

The script connects immediately and streams market data; stop it with Ctrl+C.
