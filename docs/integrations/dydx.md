# dYdX v4

dYdX is one of the largest decentralized cryptocurrency exchanges for crypto derivative products.
This integration supports live market data ingestion and order execution with dYdX v4, running on
its own Cosmos SDK application-specific blockchain (dYdX Chain) with CometBFT consensus. The order
book and matching engine run on-chain as part of the validator process. Orders are submitted as
Cosmos transactions via gRPC and settled each block. An Indexer service exposes REST and WebSocket
APIs for market data and account state.

This is the Rust-backed adapter with Python bindings.

## Installation

:::note
No additional installation extras are required. The adapter is implemented in Rust and
compiled into the core `nautilus_trader` package automatically during the build.
:::

## Examples

You can find live example scripts [here](https://github.com/nautechsystems/nautilus_trader/tree/develop/examples/live/dydx/).

## Overview

This adapter is implemented in Rust with Python bindings via PyO3. It provides direct integration
with dYdX's Indexer API (REST/WebSocket) for market data and gRPC for Cosmos SDK transaction
submission, without requiring external client libraries.

### Product support

| Product Type      | Data Feed | Trading | Notes                                  |
|-------------------|-----------|---------|----------------------------------------|
| Perpetual Futures | ✓         | ✓       | All perpetuals are USDC-settled.       |
| Spot              | -         | -       | *Not available on dYdX*.               |
| Options           | -         | -       | *Not available on dYdX*.               |

:::note
dYdX exclusively supports perpetual futures contracts. All markets are quoted in USD and settled
in USDC.
:::

## Chain architecture

Unlike centralized exchanges (CEXs) that expose a single REST/WebSocket API, dYdX v4 runs on its
own **Cosmos SDK application-specific blockchain**. This means every trade is a Cosmos transaction
that goes through consensus, and the adapter must manage sequences, gas, and block-height-based
expiration.

### Transport layers

The adapter communicates through three independent transport layers:

```
                         ┌─────────────────────────────────────────────┐
                         │              dYdX v4 Chain                  │
                         │                                             │
 ┌──────────┐  HTTP      │   ┌──────────────────────┐                  │
 │          │───────────►│   │  Indexer (read-only) │                  │
 │          │  WebSocket │   │  - REST API          │                  │
 │ Nautilus │───────────►│   │  - Streaming API     │                  │
 │ Adapter  │            │   └──────────────────────┘                  │
 │          │  gRPC      │   ┌──────────────────────┐                  │
 │          │───────────►│   │  Validator (write)   │                  │
 └──────────┘            │   │  - Cosmos Tx submit  │                  │
                         │   │  - Sequence mgmt     │                  │
                         │   └──────────────────────┘                  │
                         └─────────────────────────────────────────────┘
```

| Layer     | Target    | Direction  | Purpose                                              |
|-----------|-----------|------------|------------------------------------------------------|
| HTTP      | Indexer   | Read-only  | Instrument metadata, historical data, account state. |
| WebSocket | Indexer   | Read-only  | Real-time market data, order/fill/position updates.  |
| gRPC      | Validator | Write      | Order placement, cancellation, and batch operations. |

### Block-based settlement

dYdX blocks are produced approximately every **~0.5 seconds** (actual times vary). The adapter
includes a `BlockTimeMonitor` that tracks observed block times from the WebSocket feed to
dynamically estimate `seconds_per_block`. This estimate is used to convert time-based order expiry
into block-height offsets for short-term orders.

## Architecture

The dYdX v4 adapter includes multiple components which can be used together or separately:

- `DydxHttpClient`: Rust-backed HTTP client for Indexer REST API queries.
- `DydxWebSocketClient`: Rust-backed WebSocket client for real-time market data and account updates.
- `DydxGrpcClient`: Rust-backed gRPC client for Cosmos SDK transaction submission.
- `DydxInstrumentProvider`: Instrument parsing and loading functionality.
- `DydxDataClient`: Market data feed manager.
- `DydxExecutionClient`: Account management and trade execution gateway.
- `DydxLiveDataClientFactory`: Factory for dYdX v4 data clients (used by the trading node builder).
- `DydxLiveExecClientFactory`: Factory for dYdX v4 execution clients (used by the trading node builder).

:::note
Most users will define a configuration for a live trading node (as below),
and won't need to work with these lower level components directly.
:::

:::warning[First-time account activation]
A dYdX v4 trading account (sub-account 0) is created only after the wallet's first deposit or trade.
Until then, every gRPC/Indexer query returns `NOT_FOUND`, so `DydxExecutionClient.connect()` fails.

Before starting a live `TradingNode`, send any positive amount of USDC or other supported collateral
from the same wallet on the same network (mainnet/testnet). Once the transaction has finalised
(a few blocks), restart the node and the client will connect cleanly.
:::

## Troubleshooting

### `StatusCode.NOT_FOUND` account not found

**Cause:** The wallet/sub-account has never been funded and therefore does not yet exist on-chain.

**Fix:**

1. Deposit any positive amount of USDC to sub-account 0 on the correct network.
2. Wait for finality (roughly 30 seconds on mainnet, longer on testnet).
3. Restart the `TradingNode`; the connection should now succeed.

:::tip
In unattended deployments, wrap the `connect()` call in an exponential-backoff loop so the
client retries until the deposit appears.
:::

## Symbology

dYdX uses specific symbol conventions for perpetual futures contracts.

### Symbol format

Format: `{Base}-USD-PERP`

All perpetuals on dYdX are:

- Quoted in USD
- Settled in USDC
- Use the `.DYDX` venue suffix in Nautilus

Examples:

- `BTC-USD-PERP.DYDX` - Bitcoin perpetual futures
- `ETH-USD-PERP.DYDX` - Ethereum perpetual futures
- `SOL-USD-PERP.DYDX` - Solana perpetual futures

To subscribe in your strategy:

```python
InstrumentId.from_str("BTC-USD-PERP.DYDX")
InstrumentId.from_str("ETH-USD-PERP.DYDX")
```

:::info
The `-PERP` suffix is appended for consistency with other adapters and future-proofing. While dYdX
currently only supports perpetuals, this naming convention allows for potential expansion to other
product types.
:::

## Orders capability

dYdX supports perpetual futures trading with a full set of order types and execution
features. The Rust adapter automatically classifies orders as short-term or long-term based on
time-in-force and expiry, so no manual tagging is needed (unlike the legacy Python adapter).

### Order types

| Order Type             | Perpetuals | Notes                                              |
|------------------------|------------|----------------------------------------------------|
| `MARKET`               | ✓          | Immediate execution at best available price.       |
| `LIMIT`                | ✓          |                                                    |
| `STOP_MARKET`          | ✓          | Conditional order, always long-term.               |
| `STOP_LIMIT`           | ✓          | Conditional order, always long-term.               |
| `MARKET_IF_TOUCHED`    | ✓          | Take-profit market order, triggers on price touch. |
| `LIMIT_IF_TOUCHED`     | ✓          | Take-profit limit order, triggers on price touch.  |
| `TRAILING_STOP_MARKET` | -          | *Not supported*.                                   |

### Execution instructions

| Instruction   | Perpetuals | Notes                                                        |
|---------------|------------|--------------------------------------------------------------|
| `post_only`   | ✓          | Supported on LIMIT, STOP_LIMIT, and LIMIT_IF_TOUCHED orders. |
| `reduce_only` | ✓          | Supported on all order types except MARKET.                  |

### Time in force options

| Time in force | Perpetuals | Notes                |
|---------------|------------|----------------------|
| `GTC`         | ✓          | Good Till Canceled.  |
| `GTD`         | ✓          | Good Till Date.      |
| `FOK`         | ✓          | Fill or Kill.        |
| `IOC`         | ✓          | Immediate or Cancel. |

### Advanced order features

| Feature            | Perpetuals | Notes            |
|--------------------|------------|------------------|
| Order modification | -          | *Not supported*. |
| Bracket/OCO orders | -          | *Not supported*. |
| Iceberg orders     | -          | *Not supported*. |

### Batch operations

| Operation    | Perpetuals | Notes                                                                                                                  |
|--------------|------------|------------------------------------------------------------------------------------------------------------------------|
| Batch submit | -          | *Not supported*.                                                                                                       |
| Batch modify | -          | *Not supported*.                                                                                                       |
| Batch cancel | ✓          | Partitioned: short-term orders use `MsgBatchCancel` (single gRPC call), long-term orders use batched `MsgCancelOrder`. |

### Position management

| Feature          | Perpetuals | Notes                         |
|------------------|------------|-------------------------------|
| Query positions  | ✓          | Real-time position updates.   |
| Position mode    | -          | Netting only (see below).     |
| Leverage control | ✓          | Per-market leverage settings. |
| Margin mode      | -          | Cross margin only.            |

:::note
dYdX supports netting (one position per instrument) at the venue level. The adapter currently
operates in `NETTING` mode only. Hedging support is planned for a future version.
:::

### Order querying

| Feature              | Perpetuals | Notes                          |
|----------------------|------------|--------------------------------|
| Query open orders    | ✓          | List all active orders.        |
| Query order history  | ✓          | Historical order data.         |
| Order status updates | ✓          | Real-time order state changes. |
| Trade history        | ✓          | Execution and fill reports.    |

### Contingent orders

| Feature            | Perpetuals | Notes                                            |
|--------------------|------------|--------------------------------------------------|
| Order lists        | -          | *Not supported*.                                 |
| OCO orders         | -          | *Not supported*.                                 |
| Bracket orders     | -          | *Not supported*.                                 |
| Conditional orders | ✓          | Stop, take-profit market, and take-profit limit. |

### Order classification

dYdX classifies every order into one of three on-chain categories. The Rust adapter
automatically determines the category based on time-in-force and expiry, so no manual
configuration is required.

| Category        | Placement   | Expiry            | Typical use                                   |
|-----------------|-------------|-------------------|-----------------------------------------------|
| Short-term      | In-memory   | Block height      | IOC/FOK, or orders expiring within 20 blocks. |
| Long-term       | On-chain    | Timestamp (UTC)   | GTC/GTD with expiry beyond ~60 seconds.       |
| Conditional     | On-chain    | Timestamp (UTC)   | Stop-loss and take-profit triggers.           |

At the protocol level, **all dYdX orders are limit orders**. The `MARKET` order type
is a Nautilus convenience that the adapter implements as an aggressive IOC limit order
priced well through the book. This means market orders follow the same
`Submitted > Accepted > Filled` lifecycle as limit orders (an `OrderAccepted` event is
expected before the fill).

See the [dYdX order documentation](https://docs.dydx.exchange/api_integration-trading/short_term_vs_stateful)
for full protocol-level details on short-term vs stateful order mechanics.

#### Short-term orders

Short-term orders live **in validator memory only** and expire by block height (max 20 blocks,
roughly ~10 seconds at ~0.5s/block). They are the fastest order type on dYdX because they skip
on-chain storage.

Key properties:

- **IOC and FOK are always short-term**, regardless of other parameters
- **GTD orders** are automatically classified as short-term when the expiry falls within the
  dynamic short-term window (`20 blocks × seconds_per_block`)
- Use Good-Til-Block (GTB) for replay protection instead of Cosmos SDK sequences
- Can be broadcast **concurrently** (no semaphore, cached sequence)
- Expire silently without generating cancel events
- Cannot be batched in a single transaction (one `MsgPlaceOrder` per tx)

#### Long-term orders

Long-term (stateful) orders are **stored on-chain** and expire by UTC timestamp. They generate
explicit cancel events when they expire or are cancelled.

Key properties:

- **GTC** orders default to 90-day expiration
- **GTD** orders use the user-provided expiry timestamp
- Require proper Cosmos SDK sequence management (serialized via semaphore)
- Must be broadcast **serially** with incrementing sequence numbers
- Can be batched in a single transaction

#### Conditional orders

Conditional orders (stop-loss, take-profit) are **always stored on-chain** and triggered by
price conditions on the validator.

Key properties:

- Always use timestamp-based expiry (default 90 days for GTC)
- Always use the long-term broadcast path (serialized with semaphore)
- Include `StopMarket`, `StopLimit`, `TakeProfitMarket`, and `TakeProfitLimit`

#### Automatic routing

The adapter determines order lifetime automatically using the `BlockTimeMonitor`:

```
max_short_term_secs = SHORT_TERM_ORDER_MAXIMUM_LIFETIME (20) × seconds_per_block
```

If the order's time until expiry is within `max_short_term_secs`, it is routed as short-term.
Otherwise, it is routed as long-term. No manual configuration is needed.

#### MARKET order implementation

dYdX has no native market order type. The adapter implements `MARKET` orders as aggressive
**IOC limit orders** priced at:

- **Buy**: `oracle_price × (1 + 0.01)` (1% above oracle)
- **Sell**: `oracle_price × (1 - 0.01)` (1% below oracle)

This 1% slippage buffer (`DEFAULT_MARKET_ORDER_SLIPPAGE = 0.01`) ensures the order crosses the
spread and fills immediately, while providing price protection against extreme slippage.

### Client order ID encoding

dYdX requires `u32` client IDs on-chain, but Nautilus uses string-based `ClientOrderId` values
(e.g., `O-20260220-031943-001-000-51`). The adapter encodes these bidirectionally so that orders
can be reconciled across restarts without persisted state.

For the standard O-format (`O-YYYYMMDD-HHMMSS-TTT-SSS-CCC`), the encoding is deterministic:

| dYdX field        | Bits | Contents                                           |
|-------------------|------|----------------------------------------------------|
| `client_id`       | 32   | `[trader:10][strategy:10][count:12]` (unique key). |
| `client_metadata` | 32   | Seconds since 2020-01-01 UTC (timestamp).          |

Because the encoding is deterministic, the adapter can decode any reconciled order back to its
original `ClientOrderId` string without needing a database or mapping file.

Non-standard `ClientOrderId` formats (custom strings, plain numbers) fall back to sequential
allocation with an in-memory reverse map. These IDs can only be decoded within the same session.

#### Restart collision prevention

On restart, Nautilus resets the internal order counter based on the number of reconciled orders,
which may be lower than the highest counter value used in the previous session (e.g., if some
orders have expired from the API response). This can cause a new order to produce the same
`client_id` as a previous session's order, resulting in a duplicate venue order UUID.

The adapter prevents this by registering every `client_id` seen during reconciliation. If a new
O-format encoding produces a `client_id` that was already used, the encoder logs a warning and
falls back to sequential allocation. Sequential allocation also skips any registered values.

:::note
This protection is automatic and requires no user configuration. The warning log
`[ENCODER] client_id ... collides with reconciled order` is informational. The order will
still be submitted successfully with an alternative ID.
:::

## Broadcasting and retry strategy

### Short-term broadcast

Short-term orders use Good-Til-Block (GTB) for replay protection. The chain's `ClobDecorator`
ante handler skips Cosmos SDK sequence checking for short-term messages, so:

- **No semaphore**: broadcasts are fully concurrent
- **Cached sequence**: no increment or allocation needed
- **No retry**: if the broadcast fails, it fails immediately
- Benign cancel errors are treated as success (see below)

### Long-term broadcast

Long-term and conditional orders require proper Cosmos SDK sequence management:

- **Semaphore** with 1 permit serializes all long-term broadcasts
- **Exponential backoff**: 500ms → 1s → 2s → 4s (max 5 retries)
- **10-second total budget** prevents indefinite retry loops
- On sequence mismatch, the sequence is **resynced from chain** before retry

### Sequence mismatch detection

| Error code | Source               | Meaning                                          |
|------------|----------------------|--------------------------------------------------|
| `code=32`  | Cosmos SDK           | Account sequence mismatch                        |
| `code=104` | dYdX authenticator   | Signature verification failed (sequence-related) |

Both trigger automatic resync + retry via the `RetryManager`.

### Benign cancel errors

These errors during short-term cancel operations are treated as **success**:

| Error code  | Meaning                                                        |
|-------------|----------------------------------------------------------------|
| `code=19`   | Transaction already in mempool cache (duplicate tx)            |
| `code=9`    | Cancel already exists in memclob with >= GoodTilBlock          |
| `code=3006` | Order to cancel does not exist (already filled/expired/cancelled) |

### Batch cancel partitioning

When cancelling multiple orders, the adapter partitions them by lifetime:

1. **Short-term orders** → Single `MsgBatchCancel` via `broadcast_short_term()`
2. **Long-term orders** → Batched `MsgCancelOrder` messages via `broadcast_with_retry()`

This ensures each group uses the appropriate broadcast strategy.

## Rate limiting

### gRPC rate limiting

The adapter rate-limits gRPC `broadcast_tx` calls to prevent `ResourceExhausted` (429) errors
from validator nodes.

| Setting                       | Default | Description                               |
|-------------------------------|---------|-------------------------------------------|
| `grpc_rate_limit_per_second`  | `4`     | Maximum gRPC broadcast requests per second. Set to `None` to disable. |

### Provider limits

Known rate limits for public gRPC providers:

| Provider   | Limit              | Notes           |
|------------|--------------------|-----------------|
| Polkachu   | 300 req/min (~5/s) |                 |
| KingNodes  | 250 req/min (~4.2/s) |               |
| AutoStake  | 4 req/s            |                 |

The default of 4 req/s is conservative and works across all public providers.

### Multiple gRPC URL fallback

The adapter supports configuring multiple gRPC URLs for failover:

```python
exec_config = DydxExecClientConfig(
    base_url_grpc="https://primary-grpc.example.com:443,https://fallback-grpc.example.com:443",
    # ...
)
```

## Price and size quantization

dYdX uses integer-based quantization for prices and sizes. The adapter handles all conversions
automatically via `OrderMessageBuilder`, but understanding the parameters helps with debugging.

### Market parameters

| Parameter                      | Description                                              |
|--------------------------------|----------------------------------------------------------|
| `atomic_resolution`            | Exponent for converting human-readable size to quantums  |
| `quantum_conversion_exponent`  | Exponent for converting quantums to tokens               |
| `step_base_quantums`           | Minimum order size step in quantums                      |
| `subticks_per_tick`            | Price granularity within each tick                       |

### Market order pricing

Market orders use the oracle price with a 1% slippage buffer:

- **Buy**: `oracle_price × 1.01`
- **Sell**: `oracle_price × 0.99`

The oracle price is cached from the Indexer and refreshed periodically.

### Automatic handling

All price and size quantization is handled automatically by `OrderMessageBuilder`.
No manual conversion is needed when submitting orders through Nautilus.

## Data subscriptions

The v4 adapter supports the following data subscriptions:

| Data type           | Subscription | Historical request | Notes                                     |
|---------------------|--------------|--------------------|-------------------------------------------|
| Trade ticks         | ✓            | ✓                  |                                           |
| Quote ticks         | ✓            | -                  | Synthesized from order book top-of-book.  |
| Order book deltas   | ✓            | ✓                  | L2 depth only. Snapshot via HTTP request. |
| Bars                | ✓            | ✓                  | See supported resolutions below.          |
| Mark prices         | ✓            | -                  | Via markets channel.                      |
| Index prices        | ✓            | -                  | Via markets channel.                      |
| Funding rates       | ✓            | -                  | Via markets channel.                      |
| Instrument status   | ✓            | -                  | Via markets channel.                      |

### Supported bar resolutions

| Resolution | dYdX candle |
|------------|-------------|
| 1-MINUTE   | `1MIN`      |
| 5-MINUTE   | `5MINS`     |
| 15-MINUTE  | `15MINS`    |
| 30-MINUTE  | `30MINS`    |
| 1-HOUR     | `1HOUR`     |
| 4-HOUR     | `4HOURS`    |
| 1-DAY      | `1DAY`      |

## Subaccounts

dYdX supports multiple subaccounts per wallet address, allowing segregation of trading strategies
and risk management within a single wallet.

### Key concepts

- Each wallet address can have multiple numbered subaccounts (0, 1, 2, ..., 127).
- Subaccount 0 is the **default** and is automatically created on first deposit.
- Each subaccount maintains its own:
  - Positions
  - Open orders
  - Collateral balance
  - Margin requirements

### Configuration

Specify the subaccount number in the execution client config:

```python
config = TradingNodeConfig(
    exec_clients={
        "DYDX": DydxExecClientConfig(
            subaccount=0,  # Default subaccount
        ),
    },
)
```

:::note
Most users will use subaccount `0` (the default). Advanced users can configure multiple execution
clients for different subaccounts to implement strategy segregation or risk isolation.
:::

## Testnet setup

The dYdX testnet (`dydx-testnet-4`) is a full replica of mainnet for testing strategies
without risking real funds. All default testnet endpoints are resolved automatically when
`is_testnet=True`.

### 1. Create a testnet wallet

**Option A: Via the dYdX testnet web app (easiest)**

1. Go to [v4.testnet.dydx.exchange](https://v4.testnet.dydx.exchange)
2. Connect with MetaMask, Keplr, Phantom, or WalletConnect
3. A dYdX account is generated automatically
4. Export your secret phrase: click your address (top-right) and select "Export secret phrase"

**Option B: Use an existing secp256k1 private key**

Any 32-byte hex-encoded secp256k1 private key will work. The adapter derives the `dydx1...`
address from the key automatically using Cosmos bech32 encoding.

### 2. Fund the testnet account

A subaccount must be funded before the adapter can connect (see [First-time account activation](#architecture)).

**Via the testnet web app:**

Click the deposit/recharge button on [v4.testnet.dydx.exchange](https://v4.testnet.dydx.exchange)
to receive testnet USDC automatically.

**Via the faucet API directly:**

```bash
# Fund subaccount 0 with 2000 USDC
curl -X POST https://faucet.v4testnet.dydx.exchange/faucet/tokens \
  -H "Content-Type: application/json" \
  -d '{"address": "dydx1...", "subaccountNumber": 0, "amount": 2000}'

# Fund native tokens (for gas fees)
curl -X POST https://faucet.v4testnet.dydx.exchange/faucet/native-token \
  -H "Content-Type: application/json" \
  -d '{"address": "dydx1..."}'
```

### 3. Set environment variables

```bash
export DYDX_TESTNET_WALLET_ADDRESS="dydx1..."
export DYDX_TESTNET_PRIVATE_KEY="0x..."  # hex-encoded, 0x prefix optional
```

### 4. Configure the trading node

Set `is_testnet=True` on both data and execution clients:

```python
config = TradingNodeConfig(
    ...,  # Omitted
    data_clients={
        DYDX: DydxDataClientConfig(
            wallet_address=None,  # Falls back to DYDX_TESTNET_WALLET_ADDRESS env var
            instrument_provider=InstrumentProviderConfig(load_all=True),
            is_testnet=True,
        ),
    },
    exec_clients={
        DYDX: DydxExecClientConfig(
            wallet_address=None,  # Falls back to DYDX_TESTNET_WALLET_ADDRESS env var
            private_key=None,  # Falls back to DYDX_TESTNET_PRIVATE_KEY env var
            subaccount=0,
            instrument_provider=InstrumentProviderConfig(load_all=True),
            is_testnet=True,
        ),
    },
)
```

### Testnet endpoints

Default testnet endpoints are used automatically. Override with `base_url_*` config options if needed.

| Service   | Default URL                                          |
|-----------|------------------------------------------------------|
| HTTP      | `https://indexer.v4testnet.dydx.exchange`            |
| WebSocket | `wss://indexer.v4testnet.dydx.exchange/v4/ws`        |
| gRPC      | `https://test-dydx-grpc.kingnodes.com:443` (primary) |
| Faucet    | `https://faucet.v4testnet.dydx.exchange`             |
| Web app   | `https://v4.testnet.dydx.exchange`                   |

## Configuration

Configure the dYdX adapter through the trading node configuration. Both data and execution
clients support environment variable fallbacks for credentials and network-specific settings.

### Data client configuration options

| Option                    | Default | Description                                                                              |
|---------------------------|---------|------------------------------------------------------------------------------------------|
| `wallet_address`          | `None`  | dYdX wallet address. Falls back to `DYDX_WALLET_ADDRESS` / `DYDX_TESTNET_WALLET_ADDRESS` env var. |
| `is_testnet`              | `False` | Connect to dYdX testnet when `True`.                                                     |
| `bars_timestamp_on_close` | `True`  | Use bar close time for `ts_event` timestamps. Set `False` to use venue-native open time. |
| `base_url_http`           | `None`  | HTTP API endpoint override.                                                              |
| `base_url_ws`             | `None`  | WebSocket endpoint override.                                                             |
| `max_retries`             | `3`     | Maximum retry attempts for REST/WebSocket recovery.                                      |
| `retry_delay_initial_ms`  | `1,000`  | Initial delay (milliseconds) between retries.                                            |
| `retry_delay_max_ms`      | `10,000` | Maximum delay (milliseconds) between retries.                                            |

### Execution client configuration options

| Option                         | Default | Description                                                                                        |
|--------------------------------|---------|----------------------------------------------------------------------------------------------------|
| `wallet_address`               | `None`  | dYdX wallet address. Falls back to `DYDX_WALLET_ADDRESS` / `DYDX_TESTNET_WALLET_ADDRESS` env var. |
| `subaccount`                   | `0`     | Subaccount number (0-127). Subaccount 0 is the default.                                            |
| `private_key`                  | `None`  | Hex-encoded private key for signing. Falls back to `DYDX_PRIVATE_KEY` / `DYDX_TESTNET_PRIVATE_KEY` env var. |
| `authenticator_ids`            | `None`  | List of authenticator IDs for permissioned key trading (institutional setups).                      |
| `is_testnet`                   | `False` | Connect to dYdX testnet when `True`.                                                               |
| `base_url_http`                | `None`  | HTTP client custom endpoint override.                                                              |
| `base_url_ws`                  | `None`  | WebSocket client custom endpoint override.                                                         |
| `base_url_grpc`                | `None`  | gRPC client custom endpoint override. Supports fallback with multiple URLs.                        |
| `max_retries`                  | `3`     | Maximum retry attempts for order operations.                                                       |
| `retry_delay_initial_ms`       | `1,000`  | Initial delay (milliseconds) between retries.                                                      |
| `retry_delay_max_ms`           | `10,000` | Maximum delay (milliseconds) between retries.                                                      |
| `grpc_rate_limit_per_second`   | `4`     | Maximum gRPC requests per second. Set to `None` to disable.                                        |

### Basic setup

Configure a live `TradingNode` to include dYdX data and execution clients:

```python
from nautilus_trader.adapters.dydx import DydxDataClientConfig
from nautilus_trader.adapters.dydx import DydxExecClientConfig
from nautilus_trader.adapters.dydx.constants import DYDX
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.config import TradingNodeConfig

config = TradingNodeConfig(
    ...,  # Omitted
    data_clients={
        DYDX: DydxDataClientConfig(
            wallet_address=None,  # Falls back to env var
            instrument_provider=InstrumentProviderConfig(load_all=True),
            is_testnet=False,
        ),
    },
    exec_clients={
        DYDX: DydxExecClientConfig(
            wallet_address=None,  # Falls back to env var
            private_key=None,  # Falls back to env var
            subaccount=0,
            instrument_provider=InstrumentProviderConfig(load_all=True),
            is_testnet=False,
        ),
    },
)
```

Then, create a `TradingNode` and register the client factories:

```python
from nautilus_trader.adapters.dydx import DydxLiveDataClientFactory
from nautilus_trader.adapters.dydx import DydxLiveExecClientFactory
from nautilus_trader.adapters.dydx.constants import DYDX
from nautilus_trader.live.node import TradingNode

node = TradingNode(config=config)

node.add_data_client_factory(DYDX, DydxLiveDataClientFactory)
node.add_exec_client_factory(DYDX, DydxLiveExecClientFactory)

node.build()
```

### API credentials

Credentials can be passed directly via the Python config (`wallet_address`, `private_key`) or
resolved automatically from environment variables based on the `is_testnet` setting.

#### Environment variables

| Variable                        | Network  | Description                                    |
|---------------------------------|----------|------------------------------------------------|
| `DYDX_WALLET_ADDRESS`           | Mainnet  | Bech32-encoded wallet address (`dydx1...`).    |
| `DYDX_PRIVATE_KEY`              | Mainnet  | Hex-encoded secp256k1 private key for signing. |
| `DYDX_TESTNET_WALLET_ADDRESS`   | Testnet  | Testnet wallet address (`dydx1...`).           |
| `DYDX_TESTNET_PRIVATE_KEY`      | Testnet  | Testnet private key.                           |

#### Resolution priority

1. Value passed in the Python config (if non-empty)
2. Environment variable (selected by `is_testnet` flag)

### Permissioned key trading

#### What are API Trading Keys

API Trading Keys let you delegate trading to a separate signing key without sharing your main
wallet's seed phrase. The API key can place trades using all available margin in the owner's
cross-margin account, but cannot withdraw funds or transfer assets.

#### Creating an API key

1. In the dYdX web app, navigate to **More → API Trading Keys**
2. Click **Generate New API Key**
3. Save the **API Wallet Address** and **Private Key** (shown once, not stored by dYdX)
4. Click **Authorize API Key** (this registers the key on-chain as an authenticator)
5. The key is now active and can be used for trading

See the [dYdX API Trading Keys guide](https://help.dydx.trade/en/articles/267486-api-trading-keys-creating-a-new-key-on-the-front-end) for full details on creating and managing API keys.

#### Adapter configuration

There are two ways to configure the adapter for API Trading Key usage:

**Auto-resolution (recommended):** Set the API key's private key as `DYDX_PRIVATE_KEY` and the
owner's wallet address as `DYDX_WALLET_ADDRESS`. The adapter detects the mismatch during connect
and automatically queries the chain for matching authenticator IDs. No manual ID configuration
needed.

```python
config = DydxExecClientConfig(
    wallet_address="dydx1owner...",   # Owner account (holds margin)
    private_key="0xapikey...",         # API Trading Key private key
    # authenticator_ids resolved automatically
)
```

**Manual override:** If you know the authenticator IDs (e.g., from the dYdX TypeScript client),
pass them directly to skip auto-resolution:

```python
config = DydxExecClientConfig(
    wallet_address="dydx1owner...",
    private_key="0xapikey...",
    authenticator_ids=[1, 2],  # Skip auto-resolution
)
```

:::note
API Trading Keys only work with **cross-margin** accounts and cross markets. Isolated margin
is not supported.
:::

## Order books

Order books can be maintained at full depth or top-of-book quotes depending on the subscription.
The venue does not provide quotes directly. Instead, the adapter subscribes to order book deltas
and synthesizes quotes for the `DataEngine` when there is a top-of-book price or size change.
Only L2 (MBP) book type is supported.

## Contributing

:::info
For additional features or to contribute to the dYdX adapter, please see our
[contributing guide](https://github.com/nautechsystems/nautilus_trader/blob/develop/CONTRIBUTING.md).
:::
