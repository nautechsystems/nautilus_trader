# PancakeSwap (V2)

NautilusTrader provides a Python-facing PancakeSwap V2 adapter surface for BSC execution while keeping
execution primitives in Rust.

## Scope

This guide covers the milestone-9 Python wrapper layer:

- `PancakeSwapV2ExecClientConfig`
- `PancakeSwapV2ExecClientFactory`
- canonical defaults sourced from Rust via PyO3 (`router`, `factory`, `WBNB`)

Execution remains signer-only and config-driven.

## Installation

Build NautilusTrader with DeFi enabled:

```bash
uv sync --all-extras
uv run --active --no-sync build.py
```

## Defaults and Precedence

PancakeSwap defaults are exported from Rust and consumed in Python:

1. explicit user config values,
2. Rust per-chain defaults (`56` mainnet, `97` testnet),
3. optional unsafe overrides (must set `allow_unsafe_address_override=True`).

This avoids Rust/Python address drift.

## WBNB and Native BNB

MVP swaps are ERC20-only.

- Trade pools containing **WBNB**.
- If a wallet holds only native BNB, pre-wrap to WBNB before trading.
- Native-value swap support is not enabled in this MVP surface.

## Minimal Config

```python
from nautilus_trader.adapters.pancakeswap import PancakeSwapV2ExecClientConfig
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import TraderId

config = PancakeSwapV2ExecClientConfig(
    trader_id=TraderId("TESTER-001"),
    client_id=AccountId("SIM-001"),
    wallet_address="0x49E96E255bA418d08E66c35b588E2f2F3766E1d0",
    http_rpc_url="https://bsc.example.com",
    signer_endpoint="https://signer.example.com",
)
```

## Live-Node Wiring

```python
from nautilus_trader.adapters.pancakeswap import PancakeSwapV2ExecClientFactory
from nautilus_trader.core import nautilus_pyo3

builder = nautilus_pyo3.LiveNode.builder(
    name="PCS_V2",
    trader_id=config.trader_id,
    environment=nautilus_pyo3.Environment.SANDBOX,
)
builder = PancakeSwapV2ExecClientFactory.add_to_builder(
    builder=builder,
    config=config,
    name="BLOCKCHAIN",
)
node = builder.build()
```

See the full example at `examples/live/pancakeswap/pancakeswap_v2_swap_tester.py`.
