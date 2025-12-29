# Polymarket

Founded in 2020, Polymarket is the world’s largest decentralized prediction market platform,
enabling traders to speculate on the outcomes of world events by buying and selling binary option contracts using cryptocurrency.

NautilusTrader provides a venue integration for data and execution via Polymarket's Central Limit Order Book (CLOB) API.
The integration leverages the [official Python CLOB client library](https://github.com/Polymarket/py-clob-client)
to facilitate interaction with the Polymarket platform.

NautilusTrader supports multiple Polymarket signature types for order signing, providing flexibility for different wallet configurations.
This integration ensures that traders can execute orders securely and efficiently across various wallet types,
while NautilusTrader abstracts the complexity of signing and preparing orders for seamless execution.

## Installation

To install NautilusTrader with Polymarket support:

```bash
uv pip install "nautilus_trader[polymarket]"
```

To build from source with all extras (including Polymarket):

```bash
uv sync --all-extras
```

## Examples

You can find live example scripts [here](https://github.com/nautechsystems/nautilus_trader/tree/develop/examples/live/polymarket/).

## Binary options

A [binary option](https://en.wikipedia.org/wiki/Binary_option) is a type of financial exotic option contract in which traders bet on the outcome of a yes-or-no proposition.
If the prediction is correct, the trader receives a fixed payout; otherwise, they receive nothing.

All assets traded on Polymarket are quoted and settled in **USDC.e (PoS)**, [see below](#usdce-pos) for more information.

## Polymarket documentation

Polymarket offers comprehensive resources for different audiences:

- [Polymarket Learn](https://learn.polymarket.com/): Educational content and guides for users to understand the platform and how to engage with it.
- [Polymarket CLOB API](https://docs.polymarket.com/#introduction): Technical documentation for developers interacting with the Polymarket CLOB API.

## Overview

This guide assumes a trader is setting up for both live market data feeds and trade execution.
The Polymarket integration adapter includes multiple components, which can be used together or
separately depending on the use case.

- `PolymarketWebSocketClient`: Low-level WebSocket API connectivity (built on top of the Nautilus `WebSocketClient` written in Rust).
- `PolymarketInstrumentProvider`: Instrument parsing and loading functionality for `BinaryOption` instruments.
- `PolymarketDataClient`: A market data feed manager.
- `PolymarketExecutionClient`: A trade execution gateway.
- `PolymarketLiveDataClientFactory`: Factory for Polymarket data clients (used by the trading node builder).
- `PolymarketLiveExecClientFactory`: Factory for Polymarket execution clients (used by the trading node builder).

:::note
Most users will define a configuration for a live trading node (as below),
and won't need to necessarily work with these lower level components directly.
:::

## USDC.e (PoS)

**USDC.e** is a bridged version of USDC from Ethereum to the Polygon network, operating on Polygon's **Proof of Stake (PoS)** chain.
This enables faster, more cost-efficient transactions on Polygon while maintaining backing by USDC on Ethereum.

The contract address is [0x2791bca1f2de4661ed88a30c99a7a9449aa84174](https://polygonscan.com/address/0x2791bca1f2de4661ed88a30c99a7a9449aa84174) on the Polygon blockchain.
More information can be found in this [blog](https://polygon.technology/blog/phase-one-of-native-usdc-migration-on-polygon-pos-is-underway).

## Wallets and accounts

To interact with Polymarket via NautilusTrader, you'll need a **Polygon**-compatible wallet (such as MetaMask).

### Signature types

Polymarket supports multiple signature types for order signing and verification:

| Signature Type | Wallet Type                    | Description | Use Case |
|----------------|--------------------------------|-------------|----------|
| `0`            | EOA (Externally Owned Account) | Standard EIP712 signatures from wallets with direct private key control. | **Default.** Direct wallet connections (MetaMask, hardware wallets, etc.). |
| `1`            | Email/Magic Wallet Proxy       | Smart contract wallet for email-based accounts (Magic Link). Only the email-associated address can execute functions. | Polymarket Proxy associated with Email/Magic accounts. Requires `funder` address. |
| `2`            | Browser Wallet Proxy           | Modified Gnosis Safe (1-of-1 multisig) for browser wallets. | Polymarket Proxy associated with browser wallets. Enables UI verification. Requires `funder` address. |

:::note
See also: [Proxy wallet](https://docs.polymarket.com/developers/proxy-wallet) in the Polymarket documentation for more details about signature types and proxy wallet infrastructure.
:::

NautilusTrader defaults to signature type 0 (EOA) but can be configured to use any of the supported signature types via the `signature_type` configuration parameter.

A single wallet address is supported per trader instance when using environment variables,
or multiple wallets could be configured with multiple `PolymarketExecutionClient` instances.

:::note
Ensure your wallet is funded with **USDC.e**, otherwise you will encounter the "not enough balance / allowance" API error when submitting orders.
:::

### Setting allowances for Polymarket contracts

Before you can start trading, you need to ensure that your wallet has allowances set for Polymarket's smart contracts.
You can do this by running the provided script located at `/adapters/polymarket/scripts/set_allowances.py`.

This script is adapted from a [gist](https://gist.github.com/poly-rodr/44313920481de58d5a3f6d1f8226bd5e) created by @poly-rodr.

:::note
You only need to run this script **once** per EOA wallet that you intend to use for trading on Polymarket.
:::

This script automates the process of approving the necessary allowances for the Polymarket contracts.
It sets approvals for the USDC token and Conditional Token Framework (CTF) contract to allow the
Polymarket CLOB Exchange to interact with your funds.

Before running the script, ensure the following prerequisites are met:

- Install the web3 Python package: `uv pip install "web3==7.12.1"`.
- Have a **Polygon**-compatible wallet funded with some MATIC (used for gas fees).
- Set the following environment variables in your shell:
  - `POLYGON_PRIVATE_KEY`: Your private key for the **Polygon**-compatible wallet.
  - `POLYGON_PUBLIC_KEY`: Your public key for the **Polygon**-compatible wallet.

Once you have these in place, the script will:

- Approve the maximum possible amount of USDC (using the `MAX_INT` value) for the Polymarket USDC token contract.
- Set the approval for the CTF contract, allowing it to interact with your account for trading purposes.

:::note
You can also adjust the approval amount in the script instead of using `MAX_INT`,
with the amount specified in *fractional units* of **USDC.e**, though this has not been tested.
:::

Ensure that your private key and public key are correctly stored in the environment variables before running the script.
Here's an example of how to set the variables in your terminal session:

```bash
export POLYGON_PRIVATE_KEY="YOUR_PRIVATE_KEY"
export POLYGON_PUBLIC_KEY="YOUR_PUBLIC_KEY"
```

Run the script using:

```bash
python nautilus_trader/adapters/polymarket/scripts/set_allowances.py
```

### Script breakdown

The script performs the following actions:

- Connects to the Polygon network via an RPC URL (<https://polygon-rpc.com/>).
- Signs and sends a transaction to approve the maximum USDC allowance for Polymarket contracts.
- Sets approval for the CTF contract to manage Conditional Tokens on your behalf.
- Repeats the approval process for specific addresses like the Polymarket CLOB Exchange and Neg Risk Adapter.

This allows Polymarket to interact with your funds when executing trades and ensures smooth integration with the CLOB Exchange.

## API keys

To trade with Polymarket, you'll need to generate API credentials. Follow these steps:

1. Ensure the following environment variables are set:
   - `POLYMARKET_PK`: Your private key for signing transactions.
   - `POLYMARKET_FUNDER`: The wallet address (public key) on the **Polygon** network used for funding trades on Polymarket.

2. Run the script using:

   ```bash
   python nautilus_trader/adapters/polymarket/scripts/create_api_key.py
   ```

The script will generate and print API credentials, which you should save to the following environment variables:

- `POLYMARKET_API_KEY`
- `POLYMARKET_API_SECRET`
- `POLYMARKET_PASSPHRASE`

These can then be used for Polymarket client configurations:

- `PolymarketDataClientConfig`
- `PolymarketExecClientConfig`

## Configuration

When setting up NautilusTrader to work with Polymarket, it’s crucial to properly configure the necessary parameters, particularly the private key.

**Key parameters**:

- `private_key`: The private key for your wallet used to sign orders. The interpretation depends on your `signature_type` configuration. If not explicitly provided in the configuration, it will automatically source the `POLYMARKET_PK` environment variable.
- `funder`: The **USDC.e** wallet address used for funding trades. If not provided, will source the `POLYMARKET_FUNDER` environment variable.
- API credentials: You will need to provide the following API credentials to interact with the Polymarket CLOB:
  - `api_key`: If not provided, will source the `POLYMARKET_API_KEY` environment variable.
  - `api_secret`: If not provided, will source the `POLYMARKET_API_SECRET` environment variable.
  - `passphrase`: If not provided, will source the `POLYMARKET_PASSPHRASE` environment variable.

:::tip
We recommend using environment variables to manage your credentials.
:::

## Orders capability

Polymarket operates as a prediction market with a more limited set of order types and instructions compared to traditional exchanges.

### Order types

| Order Type             | Binary Options | Notes                               |
|------------------------|----------------|-------------------------------------|
| `MARKET`               | ✓              | **BUY orders require quote quantity**, SELL orders require base quantity. |
| `LIMIT`                | ✓              |                                     |
| `STOP_MARKET`          | -              | *Not supported by Polymarket*.      |
| `STOP_LIMIT`           | -              | *Not supported by Polymarket*.      |
| `MARKET_IF_TOUCHED`    | -              | *Not supported by Polymarket*.      |
| `LIMIT_IF_TOUCHED`     | -              | *Not supported by Polymarket*.      |
| `TRAILING_STOP_MARKET` | -              | *Not supported by Polymarket*.      |

### Quantity semantics

Polymarket interprets order quantities differently depending on the order type *and* side:

- **Limit** orders interpret `quantity` as the number of conditional tokens (base units).
- **Market SELL** orders also use base-unit quantities.
- **Market BUY** orders interpret `quantity` as quote notional in **USDC.e**.

As a result, a market buy order submitted with a base-denominated quantity will execute far more size than intended.

:::warning
When submitting market BUY orders, set `quote_quantity=True` (or pre-compute the quote-denominated amount)
and configure the execution engine with `convert_quote_qty_to_base=False` so the quote amount reaches the adapter unchanged.
The Polymarket execution client denies base-denominated market buys to prevent unintended fills.

**NautilusTrader now forwards market orders to Polymarket's native market-order endpoint, so the
quote amount you specify for a BUY is executed directly (no more synthetic max-price limits).**
:::

```python
from nautilus_trader.execution.config import ExecEngineConfig
from nautilus_trader.execution.engine import ExecutionEngine

# Temporary: disable automatic conversion until the behaviour is fully removed in a future release
config = ExecEngineConfig(convert_quote_qty_to_base=False)
engine = ExecutionEngine(msgbus=msgbus, cache=cache, clock=clock, config=config)

# Correct: Market BUY with quote quantity (spend $10 USDC)
order = strategy.order_factory.market(
    instrument_id=instrument_id,
    order_side=OrderSide.BUY,
    quantity=instrument.make_qty(10.0),
    quote_quantity=True,  # Interpret as USDC.e notional
)
strategy.submit_order(order)
```

### Execution instructions

| Instruction   | Binary Options | Notes                                    |
|---------------|----------------|------------------------------------------|
| `post_only`   | -              | *Not supported by Polymarket*.           |
| `reduce_only` | -              | *Not supported by Polymarket*.           |

### Time-in-force options

| Time in force | Binary Options | Notes                                    |
|---------------|----------------|------------------------------------------|
| `GTC`         | ✓              | Good Till Canceled.                      |
| `GTD`         | ✓              | Good Till Date.                          |
| `FOK`         | ✓              | Fill or Kill.                            |
| `IOC`         | ✓              | Immediate or Cancel (maps to FAK).       |

:::note
FAK (Fill and Kill) is Polymarket's terminology for Immediate or Cancel (IOC) semantics.
:::

### Advanced order features

| Feature            | Binary Options | Notes                               |
|--------------------|----------------|-------------------------------------|
| Order Modification | -              | Cancellation functionality only.    |
| Bracket/OCO Orders | -              | *Not supported by Polymarket*.      |
| Iceberg Orders     | -              | *Not supported by Polymarket*.      |

### Batch operations

| Operation          | Binary Options | Notes                               |
|--------------------|----------------|-------------------------------------|
| Batch Submit       | -              | *Not supported by Polymarket*.      |
| Batch Modify       | -              | *Not supported by Polymarket*.      |
| Batch Cancel       | -              | *Not supported by Polymarket*.      |

### Position management

| Feature              | Binary Options | Notes                             |
|--------------------|----------------|-------------------------------------|
| Query positions     | ✓              | Contract balance-based positions.  |
| Position mode       | -              | Binary outcome positions only.     |
| Leverage control    | -              | No leverage available.             |
| Margin mode         | -              | No margin trading.                 |

### Order querying

| Feature              | Binary Options | Notes                             |
|----------------------|----------------|-----------------------------------|
| Query open orders    | ✓              | Active orders only.               |
| Query order history  | ✓              | Limited historical data.          |
| Order status updates | ✓              | Real-time order state changes.    |
| Trade history        | ✓              | Execution and fill reports.       |

### Contingent orders

| Feature            | Binary Options | Notes                               |
|--------------------|----------------|-------------------------------------|
| Order lists        | -              | *Not supported by Polymarket*.      |
| OCO orders         | -              | *Not supported by Polymarket*.      |
| Bracket orders     | -              | *Not supported by Polymarket*.      |
| Conditional orders | -              | *Not supported by Polymarket*.      |

### Precision limits

Polymarket enforces different precision constraints based on tick size and order type.

**Binary Option instruments** typically support up to 6 decimal places for amounts (with 0.0001 tick size), but **market orders have stricter precision requirements**:

- **FOK (Fill-or-Kill) market orders:**
  - Sell orders: maker amount limited to **2 decimal places**.
  - Taker amount: limited to **4 decimal places**.
  - The product `size × price` must not exceed **2 decimal places**.

- **Regular GTC orders:** More flexible precision based on market tick size.

### Tick size precision hierarchy

| Tick Size | Price Decimals | Size Decimals | Amount Decimals |
|-----------|----------------|---------------|-----------------|
| 0.1       | 1              | 2             | 3               |
| 0.01      | 2              | 2             | 4               |
| 0.001     | 3              | 2             | 5               |
| 0.0001    | 4              | 2             | 6               |

:::note

- The tick size precision hierarchy is defined in the [`py-clob-client` `ROUNDING_CONFIG`](https://github.com/Polymarket/py-clob-client/blob/main/py_clob_client/order_builder/builder.py).
- FOK market order precision limits (2 decimals for maker amount) are based on API error responses documented in [issue #121](https://github.com/Polymarket/py-clob-client/issues/121).
- Tick sizes can change dynamically during market conditions, particularly when markets become one-sided.

:::

## Trades

Trades on Polymarket can have the following statuses:

- `MATCHED`: Trade has been matched and sent to the executor service by the operator. The executor service submits the trade as a transaction to the Exchange contract.
- `MINED`: Trade is observed to be mined into the chain, and no finality threshold is established.
- `CONFIRMED`: Trade has achieved strong probabilistic finality and was successful.
- `RETRYING`: Trade transaction has failed (revert or reorg) and is being retried/resubmitted by the operator.
- `FAILED`: Trade has failed and is not being retried.

Once a trade is initially matched, subsequent trade status updates will be received via the WebSocket.
NautilusTrader records the initial trade details in the `info` field of the `OrderFilled` event,
with additional trade events stored in the cache as JSON under a custom key to retain this information.

## Reconciliation

The Polymarket API returns either all **active** (open) orders or specific orders when queried by the
Polymarket order ID (`venue_order_id`). The execution reconciliation procedure for Polymarket is as follows:

- Generate order reports for all instruments with active (open) orders, as reported by Polymarket.
- Generate position reports from contract balances reported by Polymarket, *for instruments available in the cache*.
- Compare these reports with Nautilus execution state.
- Generate missing orders to bring Nautilus execution state in line with positions reported by Polymarket.

**Note**: Polymarket does not directly provide data for orders which are no longer active.

:::warning
An optional execution client configuration, `generate_order_history_from_trades`, is currently under development.
It is not recommended for production use at this time.
:::

## WebSockets

The `PolymarketWebSocketClient` is built on top of the high-performance Nautilus `WebSocketClient` base class, written in Rust.

### Data

The main data WebSocket handles all `market` channel subscriptions received during the initial
connection sequence, up to `ws_connection_delay_secs`. For any additional subscriptions, a new `PolymarketWebSocketClient` is
created for each new instrument (asset).

### Execution

The main execution WebSocket manages all `user` channel subscriptions based on the Polymarket instruments
available in the cache during the initial connection sequence. When trading commands are issued for additional instruments,
a separate `PolymarketWebSocketClient` is created for each new instrument (asset).

:::note
Polymarket does not support unsubscribing from channel streams once subscribed.
:::

### Subscription limits

Polymarket enforces a **maximum of 500 instruments per WebSocket connection** (undocumented limitation).

When you attempt to subscribe to 501 or more instruments on a single WebSocket connection:

- You will **not** receive the initial order book snapshot for each instrument.
- You will only receive subsequent order book updates.

To handle this limitation, NautilusTrader automatically manages WebSocket connections:

- When the subscription count exceeds 500 instruments, the adapter **automatically creates additional WebSocket connections**.
- Each connection maintains up to 500 instrument subscriptions.
- This protection ensures you receive complete order book data (including initial snapshots) for all subscribed instruments.

:::tip
If you need to subscribe to a large number of instruments (e.g., 5000+), the adapter will automatically distribute these subscriptions across multiple WebSocket connections, with each connection handling up to 500 instruments.
:::

## Limitations and considerations

The following limitations and considerations are currently known:

- Order signing via the Polymarket Python client is slow, taking around one second.
- Post-only orders are not supported.
- Reduce-only orders are not supported.

## Configuration

### Data client configuration options

| Option                          | Default           | Description |
|---------------------------------|-------------------|-------------|
| `venue`                         | `POLYMARKET`      | Venue identifier registered for the data client. |
| `private_key`                   | `None`            | Wallet private key; sourced from `POLYMARKET_PK` when omitted. |
| `signature_type`                | `0`               | Signature scheme (0 = EOA, 1 = email proxy, 2 = browser wallet proxy). |
| `funder`                        | `None`            | USDC.e funding wallet; sourced from `POLYMARKET_FUNDER` when omitted. |
| `api_key`                       | `None`            | API key; sourced from `POLYMARKET_API_KEY` when omitted. |
| `api_secret`                    | `None`            | API secret; sourced from `POLYMARKET_API_SECRET` when omitted. |
| `passphrase`                    | `None`            | API passphrase; sourced from `POLYMARKET_PASSPHRASE` when omitted. |
| `base_url_http`                 | `None`            | Override for the REST base URL. |
| `base_url_ws`                   | `None`            | Override for the WebSocket base URL. |
| `ws_connection_initial_delay_secs` | `5`           | Delay (seconds) before the first WebSocket connection to buffer subscriptions. |
| `ws_connection_delay_secs`      | `0.1`             | Delay (seconds) between subsequent WebSocket connection attempts. |
| `update_instruments_interval_mins` | `60`          | Interval (minutes) between instrument catalogue refreshes. |
| `compute_effective_deltas`      | `False`           | Compute effective order book deltas for bandwidth savings. |
| `drop_quotes_missing_side`      | `True`            | Drop quotes with missing bid/ask prices instead of substituting boundary values. |

### Execution client configuration options

| Option                           | Default      | Description |
|----------------------------------|--------------|-------------|
| `venue`                          | `POLYMARKET` | Venue identifier registered for the execution client. |
| `private_key`                    | `None`       | Wallet private key; sourced from `POLYMARKET_PK` when omitted. |
| `signature_type`                 | `0`          | Signature scheme (0 = EOA, 1 = email proxy, 2 = browser wallet proxy). |
| `funder`                         | `None`       | USDC.e funding wallet; sourced from `POLYMARKET_FUNDER` when omitted. |
| `api_key`                        | `None`       | API key; sourced from `POLYMARKET_API_KEY` when omitted. |
| `api_secret`                     | `None`       | API secret; sourced from `POLYMARKET_API_SECRET` when omitted. |
| `passphrase`                     | `None`       | API passphrase; sourced from `POLYMARKET_PASSPHRASE` when omitted. |
| `base_url_http`                  | `None`       | Override for the REST base URL. |
| `base_url_ws`                    | `None`       | Override for the WebSocket base URL. |
| `max_retries`                    | `None`       | Maximum retry attempts for submit/cancel requests. |
| `retry_delay_initial_ms`         | `None`       | Initial delay (milliseconds) between retries. |
| `retry_delay_max_ms`             | `None`       | Maximum delay (milliseconds) between retries. |
| `ack_timeout_secs`               | `5.0`        | Timeout (seconds) to wait for order/trade acknowledgment from cache. |
| `generate_order_history_from_trades` | `False` | Generate synthetic order history from trade reports when `True` (experimental). |
| `log_raw_ws_messages`            | `False`      | Log raw WebSocket payloads at INFO level when `True`. |

:::info
For additional features or to contribute to the Polymarket adapter, please see our
[contributing guide](https://github.com/nautechsystems/nautilus_trader/blob/develop/CONTRIBUTING.md).
:::

## Historical data loading

The `PolymarketDataLoader` provides methods for fetching and parsing historical market data
for research and backtesting purposes. The loader integrates with multiple Polymarket APIs to provide the required data.

:::note
All data fetching methods are **asynchronous** and must be called with `await`. The loader can optionally accept an `http_client` parameter for dependency injection (useful for testing).
:::

### Data sources

The loader fetches data from two primary sources:

1. **Polymarket Gamma API** - Market metadata, instrument details, and active market listings.
2. **Polymarket CLOB API** - Price/trade history timeseries and order book history snapshots.

### Finding markets

Use the provided utility scripts to discover active markets:

```bash
# List all active markets
python nautilus_trader/adapters/polymarket/scripts/active_markets.py

# List BTC and ETH UpDown markets specifically
python nautilus_trader/adapters/polymarket/scripts/list_updown_markets.py
```

### Basic usage

:::note
All data loader methods are **asynchronous** and must be called with `await`.
:::

```python
import asyncio
from datetime import UTC, datetime, timedelta

from nautilus_trader.adapters.polymarket import PolymarketDataLoader
from nautilus_trader.adapters.polymarket import parse_polymarket_instrument
from nautilus_trader.core.datetime import millis_to_nanos

async def load_market_data():
    # Discovery methods are static - no instance needed
    market = await PolymarketDataLoader.find_market_by_slug("fed-rate-hike-in-2025")
    condition_id = market["conditionId"]

    market_details = await PolymarketDataLoader.fetch_market_details(condition_id)
    token = market_details["tokens"][0]
    token_id = token["token_id"]

    instrument = parse_polymarket_instrument(
        market_info=market_details,
        token_id=token_id,
        outcome=token["outcome"],
    )

    return instrument, token_id

# Run the async function and create a loader bound to the instrument
instrument, token_id = asyncio.run(load_market_data())
loader = PolymarketDataLoader(instrument=instrument, token_id=token_id)
```

:::note
You can also skip the manual wiring and call `await PolymarketDataLoader.from_market_slug(...)`, which fetches the metadata and returns a loader that already has `instrument` and `token_id` set.
:::

### Fetching order book history

The `load_orderbook_snapshots()` convenience method fetches and parses order book data in one step:

```python
import pandas as pd

# Define time range
end = pd.Timestamp.now(tz="UTC")
start = end - pd.Timedelta(hours=24)

# Fetch and parse order book snapshots (automatic pagination handling)
deltas = await loader.load_orderbook_snapshots(
    start=start,
    end=end,
)
```

Alternatively, you can fetch and parse separately using the lower-level methods:

```python
# Convert timestamps to milliseconds for the API
start_time_ms = int(start.timestamp() * 1000)
end_time_ms = int(end.timestamp() * 1000)
token_id = loader.token_id

orderbook_snapshots = await loader.fetch_orderbook_history(
    token_id=token_id,
    start_time_ms=start_time_ms,
    end_time_ms=end_time_ms,
)

# Parse to NautilusTrader OrderBookDeltas
deltas = loader.parse_orderbook_snapshots(orderbook_snapshots)
```

### Fetching price history

The `load_trades()` convenience method fetches and parses trade data in one step:

```python
import pandas as pd

# Define time range
end = pd.Timestamp.now(tz="UTC")
start = end - pd.Timedelta(hours=24)

# Fetch and parse trade ticks (1-minute fidelity)
trades = await loader.load_trades(
    start=start,
    end=end,
    fidelity=1,  # 1 = 1 minute resolution
)
```

Alternatively, you can fetch and parse separately using the lower-level methods:

```python
# Convert timestamps to milliseconds for the API
start_time_ms = int(start.timestamp() * 1000)
end_time_ms = int(end.timestamp() * 1000)
token_id = loader.token_id

# Fetch raw price history
price_history = await loader.fetch_price_history(
    token_id=token_id,
    start_time_ms=start_time_ms,
    end_time_ms=end_time_ms,
    fidelity=1,  # 1 = 1 minute resolution
)

# Parse to NautilusTrader TradeTicks
trades = loader.parse_price_history(price_history)
```

:::warning
The `parse_price_history()` method creates synthetic `TradeTick` objects from price points,
as the price history endpoint doesn't include actual trade sizes. Trade sizes are set to `1.0`
and the aggressor side is inferred from price movements.
:::

### Complete backtest example

A complete working example is available at `examples/backtest/polymarket_simple_quoter.py`:

```python
import asyncio
from decimal import Decimal

import pandas as pd

from nautilus_trader.adapters.polymarket import POLYMARKET_VENUE
from nautilus_trader.adapters.polymarket import PolymarketDataLoader
from nautilus_trader.backtest.config import BacktestEngineConfig
from nautilus_trader.backtest.engine import BacktestEngine
from nautilus_trader.examples.strategies.orderbook_imbalance import OrderBookImbalance
from nautilus_trader.examples.strategies.orderbook_imbalance import OrderBookImbalanceConfig
from nautilus_trader.model.currencies import USDC_POS
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import BookType
from nautilus_trader.model.enums import OmsType
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.model.objects import Money

async def run_backtest():
    # Initialize loader and fetch market data
    loader = await PolymarketDataLoader.from_market_slug("fed-rate-hike-in-2025")
    instrument = loader.instrument

    # Fetch historical data
    start = pd.Timestamp("2025-10-30", tz="UTC")
    end = pd.Timestamp("2025-10-31", tz="UTC")

    deltas = await loader.load_orderbook_snapshots(
        start=start,
        end=end,
    )

    trades = await loader.load_trades(
        start=start,
        end=end,
    )

    # Configure and run backtest
    config = BacktestEngineConfig(trader_id=TraderId("BACKTESTER-001"))
    engine = BacktestEngine(config=config)

    engine.add_venue(
        venue=POLYMARKET_VENUE,
        oms_type=OmsType.NETTING,
        account_type=AccountType.CASH,
        base_currency=USDC_POS,
        starting_balances=[Money(10_000, USDC_POS)],
        book_type=BookType.L2_MBP,
    )

    engine.add_instrument(instrument)
    engine.add_data(deltas)
    engine.add_data(trades)

    strategy_config = OrderBookImbalanceConfig(
        instrument_id=instrument.id,
        max_trade_size=Decimal("20"),
    )

    strategy = OrderBookImbalance(config=strategy_config)
    engine.add_strategy(strategy=strategy)
    engine.run()

    # Display results
    print(engine.trader.generate_account_report(POLYMARKET_VENUE))

# Run the backtest
asyncio.run(run_backtest())
```

**Run the complete example**:

```bash
python examples/backtest/polymarket_simple_quoter.py
```

### Helper functions

The adapter provides utility functions for working with Polymarket identifiers:

```python
from nautilus_trader.adapters.polymarket import get_polymarket_instrument_id

# Create NautilusTrader InstrumentId from Polymarket identifiers
instrument_id = get_polymarket_instrument_id(
    condition_id="0x4319532e181605cb15b1bd677759a3bc7f7394b2fdf145195b700eeaedfd5221",
    token_id="60487116984468020978247225474488676749601001829886755968952521846780452448915"
)
```
