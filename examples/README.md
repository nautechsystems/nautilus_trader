# Examples

Python scripts are organized by system environment context:

- **Backtest**: Historical data with simulated venues.
- **Sandbox**: Real-time data with simulated venues.
- **Live**: `LiveNode` examples for live, sandbox, and testnet venues.
- **Other**: Various examples beyond strategies.

Scripts within each environment context directory are organized by integration.

## Live adapter examples

Rust‑native adapter testers use generic names such as `data_tester.py` and `exec_tester.py` under
`live/<adapter>/`. Adapter‑prefixed scripts in the same directories may use older APIs.

Ensure that the `nautilus_trader` package is either compiled from source or installed via pip before
running the examples. See the [installation guide](https://nautilustrader.io/docs/latest/getting_started/installation)
for more information.

From the repository root, build a Rust‑native adapter tester without connecting:

```bash
.venv/bin/python examples/live/lighter/data_tester.py --lighter-environment testnet
```
