# Bullet

[Bullet](https://bullet.xyz) is a decentralized perpetual futures exchange built on a Solana-based rollup.
It provides an on-chain order book with CEX-level performance. This integration supports
live market data ingest and order execution on Bullet.

## Overview

This adapter is implemented in Rust with Python bindings. It communicates directly with
Bullet's REST and WebSocket APIs using ed25519 signing and borsh-encoded transactions.

The Bullet adapter includes:

- `BulletHttpClient`: Low-level HTTP API connectivity (market data, account queries).
- `BulletOrderClient`: Order submission, amendment, and cancellation (signs transactions with an ed25519 key).
- `BulletWebSocketClient`: WebSocket connectivity for market data and order updates.
- `BulletInstrumentProvider`: Instrument parsing and loading from `/fapi/v1/exchangeInfo`.
- `BulletDataClient`: Live market data feed (quotes, order book deltas, mark prices, trades).
- `BulletExecutionClient`: Account management and trade execution gateway.
- `BulletLiveDataClientFactory`: Factory for Bullet data clients.
- `BulletLiveExecClientFactory`: Factory for Bullet execution clients.

## Environments

| Environment | HTTP base URL                             | WS URL                                    |
| :---------- | :---------------------------------------- | :---------------------------------------- |
| Mainnet     | `https://tradingapi.bullet.xyz`           | `wss://tradingapi.bullet.xyz/ws`          |
| Testnet     | `https://tradingapi.testnet.bullet.xyz`   | `wss://tradingapi.testnet.bullet.xyz/ws`  |
| Staging     | `https://tradingapi.staging.bullet.xyz`   | `wss://tradingapi.staging.bullet.xyz/ws`  |

The default environment is **testnet**. Set `base_url_http` / `base_url_ws` in the config
or use `BULLET_BASE_URL` to override.

## Key signing

Bullet uses **ed25519** key pairs. Trading API calls are signed with a **delegate key**
authorized to trade on behalf of your main account. If no `account_address` is configured,
the signing key is used directly (direct-trading mode).

Supported key formats:

- **Base58 string** — `BULLET_PRIVATE_KEY` environment variable or `private_key` config field.
- **Solana JSON keystore** — `BULLET_KEY_FILE` env or `key_file` config field (a JSON array of 64 bytes).

Resolution order: `key_file` config → `BULLET_KEY_FILE` env → `BULLET_PRIVATE_KEY` env → `private_key` config.

## Configuration

```python
from nautilus_trader.adapters.bullet.config import BulletDataClientConfig, BulletExecClientConfig

data_config = BulletDataClientConfig(
    base_url_http="https://tradingapi.testnet.bullet.xyz",
    base_url_ws="wss://tradingapi.testnet.bullet.xyz/ws",
)

exec_config = BulletExecClientConfig(
    # Key — one of:
    private_key="<base58-or-hex-ed25519-key>",  # or
    key_file="~/.config/bullet/id.json",
    # Optional: main account if using a delegate key
    account_address="<base58-main-account-address>",
    base_url_http="https://tradingapi.testnet.bullet.xyz",
    base_url_ws="wss://tradingapi.testnet.bullet.xyz/ws",
)
```

### Environment variables

| Variable                | Description                                       |
| :---------------------- | :------------------------------------------------ |
| `BULLET_PRIVATE_KEY`    | Base58 or hex ed25519 private key (delegate)      |
| `BULLET_KEY_FILE`       | Path to Solana-compatible JSON keystore           |
| `BULLET_ACCOUNT_ADDRESS`| Main account address (delegate-key mode)          |
| `BULLET_BASE_URL`       | Override HTTP base URL                            |
| `BULLET_SYMBOL`         | Override instrument (e.g. `ETH-USD-PERP.BULLET`)  |

## Symbol convention

Bullet native symbols are `{BASE}-{QUOTE}` (e.g. `SOL-USD`, `BTC-USD`).
NautilusTrader instrument IDs are `{BASE}-{QUOTE}-PERP.BULLET` (e.g. `SOL-USD-PERP.BULLET`).

## Examples

You can find live example scripts [here](https://github.com/nautechsystems/nautilus_trader/tree/develop/examples/live/bullet/).

### Testnet market maker

```bash
BULLET_KEY_FILE=~/.config/bullet/id.json \
uv run python examples/live/bullet/bullet_node_market_maker.py
```

### Exec tester (place → amend → cancel)

```bash
BULLET_KEY_FILE=~/.config/bullet/id.json \
uv run python examples/live/bullet/bullet_exec_tester.py
```

### Fill tester (market-crossing order)

```bash
BULLET_KEY_FILE=~/.config/bullet/id.json \
uv run python examples/live/bullet/bullet_fill_tester.py
```
