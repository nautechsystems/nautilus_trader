# OKX

Founded in 2017, OKX is a leading cryptocurrency exchange offering spot, perpetual swap,
futures, and options trading. This integration supports live market data ingest and order
execution on OKX.

## Overview

This adapter is implemented in Rust, with optional Python bindings for ease of use in Python-based workflows.
It does not require external OKX client libraries; the core components are compiled as a static library and linked automatically during the build.

## Examples

You can find live example scripts [here](https://github.com/nautechsystems/nautilus_trader/tree/develop/examples/live/okx/).

### Product support

| Product Type      | Data Feed | Trading | Notes                                            |
|-------------------|-----------|---------|--------------------------------------------------|
| Spot              | ✓         | ✓       | Use for index prices.                            |
| Perpetual Swaps   | ✓         | ✓       | Linear and inverse contracts.                    |
| Futures           | ✓         | ✓       | Specific expiration dates.                       |
| Margin            | ✓         | ✓       | Spot trading with margin/leverage (spot margin). |
| Options           | ✓         | ✓       | Limit orders only; no market or conditional.     |

:::note
**Options support**: The adapter supports options market data, venue-provided Greeks
(`subscribe_option_greeks`), and order execution for options instruments. See the
[Options trading](#options-trading) section below for details and the
[Options](../concepts/options.md) guide for subscription patterns.
:::

:::info
**Instrument multipliers**: For derivatives (SWAP, FUTURES, OPTIONS), instrument multipliers are calculated as the product of OKX's `ctMult` (contract multiplier) and `ctVal` (contract value) fields. This ensures position sizing accurately reflects both the contract size and value.
:::

The OKX adapter includes multiple components, which can be used separately or together depending on your use case.

- `OKXHttpClient`: Low-level HTTP API connectivity.
- `OKXWebSocketClient`: Low-level WebSocket API connectivity.
- `OKXInstrumentProvider`: Instrument parsing and loading functionality.
- `OKXDataClient`: Market data feed manager.
- `OKXExecutionClient`: Account management and trade execution gateway.
- `OKXLiveDataClientFactory`: Factory for OKX data clients (used by the trading node builder).
- `OKXLiveExecClientFactory`: Factory for OKX execution clients (used by the trading node builder).

:::note
Most users will define a configuration for a live trading node (as shown below),
and won’t need to work directly with these lower-level components.
:::

## Symbology

OKX uses specific symbol conventions for different instrument types. All instrument IDs should include the `.OKX` suffix when referencing them (e.g., `BTC-USDT.OKX` for spot Bitcoin).

### Symbol format by instrument type

#### SPOT

Format: `{BaseCurrency}-{QuoteCurrency}`

Examples:

- `BTC-USDT` - Bitcoin against USDT (Tether)
- `BTC-USDC` - Bitcoin against USDC
- `ETH-USDT` - Ethereum against USDT
- `SOL-USDT` - Solana against USDT

To subscribe to spot Bitcoin USD in your strategy:

```python
InstrumentId.from_str("BTC-USDT.OKX")  # For USDT-quoted spot
InstrumentId.from_str("BTC-USDC.OKX")  # For USDC-quoted spot
```

#### SWAP (Perpetual Futures)

Format: `{BaseCurrency}-{QuoteCurrency}-SWAP`

Examples:

- `BTC-USDT-SWAP` - Bitcoin perpetual swap (linear, USDT-margined)
- `BTC-USD-SWAP` - Bitcoin perpetual swap (inverse, coin-margined)
- `ETH-USDT-SWAP` - Ethereum perpetual swap (linear)
- `ETH-USD-SWAP` - Ethereum perpetual swap (inverse)

Linear vs Inverse contracts:

- **Linear** (USDT-margined): Uses stablecoins like USDT as margin.
- **Inverse** (coin-margined): Uses the base cryptocurrency as margin.

#### FUTURES (Dated Futures)

Format: `{BaseCurrency}-{QuoteCurrency}-{YYMMDD}`

Examples:

- `BTC-USD-251226` - Bitcoin futures expiring December 26, 2025
- `ETH-USD-251226` - Ethereum futures expiring December 26, 2025
- `BTC-USD-250328` - Bitcoin futures expiring March 28, 2025

Note: Futures are typically inverse contracts (coin-margined).

#### OPTIONS

Format: `{BaseCurrency}-{QuoteCurrency}-{YYMMDD}-{Strike}-{Type}`

Examples:

- `BTC-USD-251226-100000-C` - Bitcoin call option, $100,000 strike, expiring December 26, 2025
- `BTC-USD-251226-100000-P` - Bitcoin put option, $100,000 strike, expiring December 26, 2025
- `ETH-USD-251226-4000-C` - Ethereum call option, $4,000 strike, expiring December 26, 2025

Where:

- `C` = Call option
- `P` = Put option

### Common questions

**Q: How do I subscribe to spot Bitcoin USD?**
A: Use `BTC-USDT.OKX` for USDT-margined spot or `BTC-USDC.OKX` for USDC-margined spot.

**Q: What's the difference between BTC-USDT-SWAP and BTC-USD-SWAP?**
A: `BTC-USDT-SWAP` is a linear perpetual (USDT-margined), while `BTC-USD-SWAP` is an inverse perpetual (BTC-margined).

**Q: How do I know which contract type to use?**
A: Check the `contract_types` parameter in the configuration:

- For linear contracts: `OKXContractType.LINEAR`.
- For inverse contracts: `OKXContractType.INVERSE`.

## Orders capability

Below are the order types, execution instructions, and time-in-force options supported
for linear perpetual swap products on OKX.

### WebSocket order identification

OKX WebSocket order operations use `instIdCode` (a numeric instrument identifier)
instead of the string `instId` parameter. The adapter resolves `instIdCode` values
from the instrument definitions fetched during startup and caches them for the
session lifetime. If the instrument cache is empty (e.g. because of a failed
bootstrap), order submissions fail with a clear error.

### Client order ID requirements

:::note
OKX has specific requirements for client order IDs:

- **No hyphens allowed**: OKX does not accept hyphens (`-`) in client order IDs.
- Maximum length: 32 characters.
- Allowed characters: alphanumeric characters and underscores only.

When configuring your strategy, ensure you set:

```python
use_hyphens_in_client_order_ids=False
```

:::

### Order types

| Order Type          | Linear Perpetual Swap | Notes                                                         |
|---------------------|-----------------------|---------------------------------------------------------------|
| `MARKET`            | ✓                     | Immediate execution at market price. Supports quote quantity. |
| `MARKET_TO_LIMIT`   | ✓                     | Market order converted to IOC limit.                          |
| `LIMIT`             | ✓                     | Execution at specified price or better.                       |
| `STOP_MARKET`       | ✓                     | Conditional market order (OKX algo order).                    |
| `STOP_LIMIT`        | ✓                     | Conditional limit order (OKX algo order).                     |
| `MARKET_IF_TOUCHED` | ✓                     | Conditional market order (OKX algo order).                    |
| `LIMIT_IF_TOUCHED`  | ✓                     | Conditional limit order (OKX algo order).                     |
| `TRAILING_STOP_MARKET` | ✓                  | Trailing stop market order (OKX advance algo order).          |

:::info
**Conditional orders**: `STOP_MARKET`, `STOP_LIMIT`, `MARKET_IF_TOUCHED`, `LIMIT_IF_TOUCHED`, and `TRAILING_STOP_MARKET` are implemented as OKX algo orders, providing advanced trigger capabilities with multiple price sources. `TRAILING_STOP_MARKET` uses OKX's advance algo order API (`move_order_stop`) and requires the separate `cancel-advance-algos` endpoint for cancellation.
:::

### Quantity semantics for spot margin trading

When using spot margin trading (`use_spot_margin=True`), OKX interprets order quantities differently depending on the order side:

- **Limit** orders interpret `quantity` as the number of base currency units.
- **Market SELL** orders also use base-unit quantities.
- **Market BUY** orders interpret `quantity` as quote notional (e.g., USDT).

:::warning
**When submitting spot margin market BUY orders**, set `quote_quantity=True` on the order (or
pre-compute the quote-denominated amount). The OKX execution client denies base-denominated
market buy orders for spot margin to prevent unintended fills.

**On the first fill**, the order quantity is automatically updated from the quote quantity to the
actual base quantity received, reflecting the executed trade.
:::

```python
# Spot margin market BUY with quote quantity (spend $100 USDT)
order = strategy.order_factory.market(
    instrument_id=instrument_id,
    order_side=OrderSide.BUY,
    quantity=instrument.make_qty(100.0),
    quote_quantity=True,  # Interpret as USDT notional
)
strategy.submit_order(order)
```

### Execution instructions

| Instruction    | Linear Perpetual Swap | Notes                  |
|----------------|-----------------------|------------------------|
| `post_only`    | ✓                     | Only for LIMIT orders. |
| `reduce_only`  | ✓                     | Only for derivatives.  |

### Time in force

| Time in force | Linear Perpetual Swap | Notes                                             |
|---------------|-----------------------|---------------------------------------------------|
| `GTC`         | ✓                     | Good Till Canceled.                               |
| `FOK`         | ✓                     | Fill or Kill.                                     |
| `IOC`         | ✓                     | Immediate or Cancel.                              |
| `GTD`         | -                     | *Not supported by OKX API.*                       |

:::note
**GTD (Good Till Date) time in force**: OKX does not support native GTD functionality through their API.

If you need GTD functionality, you must use Nautilus's strategy-managed GTD feature, which will handle the order expiration by canceling the order at the specified expiry time.
:::

### Batch operations

| Operation          | Linear Perpetual Swap | Notes                                     |
|--------------------|-----------------------|-------------------------------------------|
| Batch Submit       | ✓                     | Submit multiple orders in single request. |
| Batch Modify       | ✓                     | Modify multiple orders in single request. |
| Batch Cancel       | ✓                     | Cancel multiple orders in single request. |

### Position management

| Feature           | Linear Perpetual Swap | Notes                                                |
|-------------------|-----------------------|------------------------------------------------------|
| Query positions   | ✓                     | Real‑time position updates.                          |
| Position mode     | ✓                     | Net vs Long/Short mode (see below).                  |
| Leverage control  | ✓                     | Dynamic leverage adjustment per instrument.          |
| Margin mode       | ✓                     | Supports cash, isolated, and cross modes.            |

#### Position modes

OKX supports two position modes for derivatives trading:

- **Net mode** (Netting): Single position per instrument that can be positive (LONG) or negative (SHORT). Buy and sell orders net against each other. This is the default and recommended for most traders.
- **Long/Short mode** (Hedging): Separate long and short positions for the same instrument. Allows simultaneous long and short positions, useful for hedging strategies.

:::note
Position mode must be configured via the OKX Web/App interface and applies account-wide. The adapter automatically detects the current position mode and handles position reporting accordingly.
:::

### Trade modes and margin configuration

OKX's unified account system supports different trade modes for spot and derivatives trading. The adapter automatically determines the correct trade mode based on your configuration and instrument type.

:::note
**Important**: Account modes must be initially configured via the OKX Web/App interface. The API cannot set the account mode for the first time.
:::

For more details on OKX's account modes and margin system, see the [OKX Account Mode documentation](https://www.okx.com/docs-v5/en/#overview-account-mode).

#### Trade modes overview

OKX supports four trade modes, which the adapter selects automatically based on your configuration:

| Mode                | Used For                                   | Leverage | Borrowing | Configuration |
|---------------------|--------------------------------------------|----------|-----------|---------------|
| **`cash`**          | Simple spot trading                        | -        | -         | `use_spot_margin=False` (default for SPOT) |
| **`isolated`**      | Spot margin or derivatives (default)       | ✓        | ✓         | `use_spot_margin=True` with `margin_mode=ISOLATED` (or unset) for SPOT; default for derivatives |
| **`cross`**         | Spot margin or derivatives, shared pool    | ✓        | ✓         | `use_spot_margin=True` with `margin_mode=CROSS` for SPOT; `margin_mode=CROSS` for derivatives |

#### Configuration-based trade mode selection

**The adapter automatically selects the correct trade mode** based on:

1. **Instrument type** (SPOT vs derivatives)
2. **Configuration settings** (`use_spot_margin` for SPOT, `margin_mode` for derivatives)

##### For SPOT trading

```python
# Simple SPOT trading without leverage (uses 'cash' mode)
exec_clients={
    OKX: OKXExecClientConfig(
        instrument_types=(OKXInstrumentType.SPOT,),
        use_spot_margin=False,  # Default - simple SPOT
        # ... other config
    ),
}

# SPOT trading WITH margin/leverage (uses 'isolated' or 'cross' mode)
exec_clients={
    OKX: OKXExecClientConfig(
        instrument_types=(OKXInstrumentType.SPOT,),
        use_spot_margin=True,  # Enable margin trading for SPOT
        margin_mode=OKXMarginMode.ISOLATED,  # Or CROSS for shared margin
        # ... other config
    ),
}
```

##### For derivatives trading (SWAP/FUTURES/OPTIONS)

```python
# Derivatives with isolated margin (default - uses 'isolated' mode)
exec_clients={
    OKX: OKXExecClientConfig(
        instrument_types=(OKXInstrumentType.SWAP,),
        margin_mode=OKXMarginMode.ISOLATED,  # Or omit - ISOLATED is default
        # ... other config
    ),
}

# Derivatives with cross margin (uses 'cross' mode)
exec_clients={
    OKX: OKXExecClientConfig(
        instrument_types=(OKXInstrumentType.SWAP,),
        margin_mode=OKXMarginMode.CROSS,  # Share margin across all positions
        # ... other config
    ),
}
```

##### For mixed SPOT and derivatives trading

When trading both SPOT and derivatives instruments simultaneously, the adapter automatically determines the correct trade mode **per-order** based on the instrument being traded:

```python
# Mixed SPOT + SWAP configuration
exec_clients={
    OKX: OKXExecClientConfig(
        instrument_types=(OKXInstrumentType.SPOT, OKXInstrumentType.SWAP),
        use_spot_margin=True,           # Applies to SPOT orders only
        margin_mode=OKXMarginMode.CROSS,  # Applies to SWAP orders only
        # ... other config
    ),
}
```

**How it works:**

- **SPOT orders** → Uses `cross` mode (because `use_spot_margin=True` and `margin_mode=CROSS`)
- **SWAP orders** → Uses `cross` mode (because `margin_mode=CROSS`)
- Each order automatically gets the correct `tdMode` based on its instrument type
- No manual intervention required

This enables strategies that trade across multiple instrument types with different margin configurations, such as:

- Spot-futures arbitrage strategies
- Delta-neutral strategies combining spot and perpetual swaps
- Market making across spot and derivatives markets

:::warning
**Manual trade mode override**: While you can still manually override the trade mode per order using `params={"td_mode": "..."}`, this is **not recommended** as it bypasses automatic mode selection and can lead to order rejection if the wrong mode is specified for the instrument type (e.g., using `isolated` for SPOT instruments).

Only use manual override if you have specific requirements that cannot be met through configuration.
:::

#### Benefits of configuration-based approach

- **Type-safe**: Configuration is validated at startup before placing any orders.
- **Automatic**: System chooses correct mode based on instrument type and intent.
- **Clear**: Field names explain purpose (`use_spot_margin` vs obscure `td_mode` parameter).
- **Safe**: Impossible to use incompatible combinations (e.g., `isolated` mode for SPOT).
- **Backwards compatible**: Default values maintain existing behavior.

### Order querying

| Feature              | Linear Perpetual Swap | Notes                                     |
|----------------------|-----------------------|-------------------------------------------|
| Query open orders    | ✓                     | List all active orders.                   |
| Query order history  | ✓                     | Historical order data.                    |
| Order status updates | ✓                     | Real‑time order state changes.            |
| Trade history        | ✓                     | Execution and fill reports.               |

### Contingent orders

| Feature             | Linear Perpetual Swap | Notes                                      |
|---------------------|-----------------------|--------------------------------------------|
| Order lists         | ✓                     | Batch via WS; regular orders only.         |
| OCO orders          | ✓                     | One‑Cancels‑Other orders.                  |
| Bracket orders      | ✓                     | Stop loss + take profit combinations.      |
| Conditional orders  | ✓                     | Stop and limit‑if‑touched orders.          |

#### Conditional order architecture

Conditional orders (OKX algo orders) use a hybrid architecture for optimal performance and reliability:

- **Submission**: Via HTTP REST API (`/api/v5/trade/order-algo`)
- **Status updates**: Via WebSocket business endpoint (`/ws/v5/business`) on the `orders-algo` channel
- **Cancellation**: Via HTTP REST API using algo order ID tracking

This design ensures:

- Immediate submission acknowledgment through HTTP.
- Real-time status updates through WebSocket.
- Proper order lifecycle management with algo order ID mapping.

#### Supported conditional order types

| Order Type          | Trigger Types          | Notes                                     |
|---------------------|------------------------|-------------------------------------------|
| `STOP_MARKET`       | Last, Mark, Index      | Market execution when triggered.          |
| `STOP_LIMIT`        | Last, Mark, Index      | Limit order placement when triggered.     |
| `MARKET_IF_TOUCHED` | Last, Mark, Index      | Market execution when price touched.      |
| `LIMIT_IF_TOUCHED`  | Last, Mark, Index      | Limit order placement when price touched. |
| `TRAILING_STOP_MARKET` | Last, Mark, Index   | Trailing stop with callback ratio.        |

#### Trigger price types

Conditional orders support different trigger price sources:

- **Last Price** (`TriggerType.LAST_PRICE`): Uses the last traded price (default).
- **Mark Price** (`TriggerType.MARK_PRICE`): Uses the mark price (recommended for derivatives).
- **Index Price** (`TriggerType.INDEX_PRICE`): Uses the underlying index price.

```python
# Example: Stop loss using mark price trigger
stop_order = order_factory.stop_market(
    instrument_id=instrument_id,
    order_side=OrderSide.SELL,
    quantity=Quantity.from_str("0.1"),
    trigger_price=Price.from_str("45000.0"),
    trigger_type=TriggerType.MARK_PRICE,  # Use mark price for trigger
)
strategy.submit_order(stop_order)
```

## Risk management

### Liquidation and ADL event handling

The OKX adapter automatically detects and handles exchange-initiated risk management events:

- **Liquidation orders**: When a position is liquidated by the exchange (full or partial), the adapter detects the liquidation category and logs warnings with order details. These orders are processed normally through the order and fill pipeline.
- **Auto-Deleveraging (ADL)**: When your position is closed by the exchange to offset a counterparty's liquidation, the adapter detects and logs the ADL event with position details.

Detection is driven by the `category` field on the order record. The
recognised values are:

| `category`              | Meaning                                              |
|-------------------------|------------------------------------------------------|
| `full_liquidation`      | Full position liquidation.                           |
| `partial_liquidation`   | Partial position liquidation.                        |
| `adl`                   | Auto‑deleveraging close.                             |
| `delivery`              | Contract delivery at expiry.                         |
| `normal` / other values | Regular order flow.                                  |

Detection runs on both paths:

- WebSocket `orders` channel (live order/fill updates).
- HTTP `GET /api/v5/trade/orders-history` and `orders-history-archive`
  (used during reconciliation and cold-start mass status).

:::info
**Liquidation and ADL events are logged at WARNING level** with details including order ID, instrument, and state. Monitor your logs for these events as part of your risk management process.

The adapter handles these exchange-generated orders, generating appropriate `OrderFilled` events and updating positions accordingly. No special handling is required in your strategy code.
:::

Upstream references:

- [Order channel and `category` field](https://www.okx.com/docs-v5/en/#order-book-trading-trade-ws-order-channel)
- [Auto-Deleveraging mechanism](https://www.okx.com/help/okx-contract-auto-deleveraging-adl)
- [Liquidation mechanism](https://www.okx.com/help/introduction-to-liquidation)

## Options trading

The OKX adapter supports trading options (OPTION instrument type) with some differences from
other derivatives. OKX options are inverse contracts settled in the underlying cryptocurrency.
For full API details see the
[OKX Options Trading documentation](https://www.okx.com/docs-v5/en/#order-book-trading-trade-post-place-order).

### Supported order types

Only limit-style orders are supported. OKX does not allow market orders for options.

| Order Type | Supported | Notes                                             |
|------------|-----------|---------------------------------------------------|
| `LIMIT`    | ✓         | Standard limit order.                             |
| `MARKET`   | -         | Rejected by the adapter before reaching the API.  |

Options support FOK and IOC time-in-force. OKX uses a dedicated `op_fok` order type for
options FOK orders; the adapter handles this mapping automatically.

Conditional/algo orders (`STOP_MARKET`, `STOP_LIMIT`, `MARKET_IF_TOUCHED`,
`LIMIT_IF_TOUCHED`, `TRAILING_STOP_MARKET`) are not supported for options and will be denied.

### Pricing modes

Options orders can be priced in three mutually exclusive ways. Pass the pricing mode via
order `params`:

| Mode  | Parameter | Description                                             |
|-------|-----------|---------------------------------------------------------|
| Price | (default) | Standard limit price in the contract's currency.        |
| USD   | `px_usd`  | Price in USD terms.                                     |
| IV    | `px_vol`  | Price in implied volatility (1.0 = 100%).               |

```python
# Price in USD
order = strategy.order_factory.limit(
    instrument_id=InstrumentId.from_str("BTC-USD-250328-50000-C.OKX"),
    order_side=OrderSide.BUY,
    quantity=Quantity.from_int(1),
    price=Price.from_str("0"),  # Placeholder; px_usd takes precedence
    params={"px_usd": "100.5"},
)

# Price in implied volatility
order = strategy.order_factory.limit(
    instrument_id=InstrumentId.from_str("BTC-USD-250328-50000-C.OKX"),
    order_side=OrderSide.BUY,
    quantity=Quantity.from_int(1),
    price=Price.from_str("0"),  # Placeholder; px_vol takes precedence
    params={"px_vol": "0.55"},
)
```

When modifying an order, the same `px_usd` or `px_vol` params can be passed to the modify
command to amend the price in the original pricing mode.

### Option Greeks

OKX publishes two parallel greek sets on the `opt-summary` channel:

- **Black-Scholes (`BLACK_SCHOLES`)**: greeks denominated in USD. Matches the convention used
  by the Deribit and Bybit adapters.
- **Price-adjusted (`PRICE_ADJUSTED`)**: greeks denominated in the underlying/coin units.
  Matches OKX's native contract convention.

By default the adapter emits **both** on every `opt-summary` tick. Each emitted `OptionGreeks`
carries a `convention` field set to `GreeksConvention.BLACK_SCHOLES` or
`GreeksConvention.PRICE_ADJUSTED`, so receivers can branch per message.

To narrow the stream, pass `params["greeks_convention"]` on subscribe:

- Single string: `"BLACK_SCHOLES"` or `"PRICE_ADJUSTED"` (case-insensitive)
- List of strings: `["BLACK_SCHOLES", "PRICE_ADJUSTED"]`
- Omitted: adapter emits both

Unknown entries log a warning and are skipped. If every requested entry is unknown, the
adapter falls back to emitting both.

```python
# Default (both conventions, receiver branches)
self.subscribe_option_greeks(instrument_id)

def on_option_greeks(self, greeks: OptionGreeks) -> None:
    if greeks.convention == GreeksConvention.BLACK_SCHOLES:
        self._handle_bs(greeks)
    else:
        self._handle_pa(greeks)
```

```python
# Single-convention narrowing
self.subscribe_option_greeks(
    instrument_id,
    params={"greeks_convention": "PRICE_ADJUSTED"},
)
```

```python
# Explicit list (equivalent to the default when both are listed)
self.subscribe_option_greeks(
    instrument_id,
    params={"greeks_convention": ["BLACK_SCHOLES", "PRICE_ADJUSTED"]},
)
```

:::note
The data engine deduplicates option-greeks subscriptions by `instrument_id`, so if two actors
on one node subscribe to the same instrument with different single conventions only the first
one reaches the adapter. The second actor gets the first actor's convention set. Workaround:
either actor can subscribe without `params` (or with the full list) to receive both streams
and filter locally on `greeks.convention`.
:::

### Position Greeks

The adapter exposes position-level Black-Scholes Greeks (`delta_bs`, `gamma_bs`, `theta_bs`,
`vega_bs`) from OKX position data. These are available through the standard position reporting
pipeline.

### Restrictions

- `reduce_only` is not applicable to options and is automatically stripped.
- Position side defaults to `Net`.

### Configuration

Options require the `instrument_families` config parameter to scope which underlyings to load:

```python
config = TradingNodeConfig(
    data_clients={
        OKX: OKXDataClientConfig(
            instrument_types=(OKXInstrumentType.OPTION,),
            instrument_families=("BTC-USD", "ETH-USD"),
            instrument_provider=InstrumentProviderConfig(load_all=True),
        ),
    },
    exec_clients={
        OKX: OKXExecClientConfig(
            instrument_types=(OKXInstrumentType.OPTION,),
            instrument_families=("BTC-USD", "ETH-USD"),
            margin_mode=OKXMarginMode.CROSS,
        ),
    },
)
```

## Authentication

To use the OKX adapter, you'll need to create API credentials in your OKX account:

1. Log into your OKX account and navigate to the API management page.
2. Create a new API key with the required permissions for trading and data access.
3. Note down your API key, secret key, and passphrase.

You can provide these credentials through environment variables:

```bash
export OKX_API_KEY="your_api_key"
export OKX_API_SECRET="your_api_secret"
export OKX_API_PASSPHRASE="your_passphrase"
```

Or pass them directly in the configuration (not recommended for production).

## Demo trading

OKX provides a demo trading environment for testing strategies without real funds.

### Setting up a demo account

1. Log into your OKX account at [okx.com](https://www.okx.com).
2. Navigate to **Trade** → **Demo Trading**.
3. Go to **Personal Center** within Demo Trading.
4. Select **Demo Trading API** and create a new API key.
5. Note down your demo API key, secret, and passphrase.

You can provide demo credentials through environment variables:

```bash
export OKX_API_KEY="your_demo_api_key"
export OKX_API_SECRET="your_demo_api_secret"
export OKX_API_PASSPHRASE="your_demo_passphrase"
```

### Configuration

Set `is_demo=True` in your client configuration:

```python
config = TradingNodeConfig(
    data_clients={
        OKX: OKXDataClientConfig(
            is_demo=True,  # Enable demo mode
            # ... other config
        ),
    },
    exec_clients={
        OKX: OKXExecClientConfig(
            is_demo=True,  # Enable demo mode
            # ... other config
        ),
    },
)
```

When demo mode is enabled:

- REST API requests include the `x-simulated-trading: 1` header.
- WebSocket connections use demo endpoints (`wspap.okx.com`).

:::note
Demo API keys are separate from production keys. You must create API keys specifically for demo trading through the Demo Trading interface. Production API keys will not work in demo mode.
:::

## Funding rates

The adapter receives funding rate data from the
[Funding Rate Channel](https://www.okx.com/docs-v5/en/#public-data-websocket-funding-rate-channel)
WebSocket stream. OKX provides both `fundingTime` and `nextFundingTime` in each message,
and the adapter computes `interval` as the difference between these two values.

For historical funding rate requests, the adapter computes the interval from consecutive
funding timestamps returned by the
[Get Funding Rate History](https://www.okx.com/docs-v5/en/#public-data-rest-api-get-funding-rate-history)
endpoint.

## Rate limiting

The adapter enforces OKX’s per-endpoint quotas while keeping sensible defaults for both REST and WebSocket calls.

### REST limits

- Global cap: 250 requests per second (matches 500 requests / 2 seconds IP allowance).
- Endpoint-specific quotas appear in the table below and mirror OKX’s published limits where available.

### WebSocket limits

- Connection establishment: 3 requests per second (per IP).
- Subscription operations (subscribe/unsubscribe/login): 480 requests per hour per connection.
- Order actions (place/cancel/amend): 250 requests per second.

:::warning
OKX enforces per-endpoint and per-account quotas; exceeding them leads to HTTP 429 responses and temporary throttling on that key.
:::

| Key / Endpoint                   | Limit (req/sec) | Notes                                                   |
|----------------------------------|-----------------|---------------------------------------------------------|
| `okx:global`                     | 250             | Matches 500 req / 2 s IP allowance.                     |
| `/api/v5/public/instruments`     | 10              | Matches OKX 20 req / 2 s docs.                          |
| `/api/v5/market/candles`         | 50              | Higher allowance for streaming candles.                 |
| `/api/v5/market/history-candles` | 20              | Conservative quota for large historical pulls.          |
| `/api/v5/market/history-trades`  | 30              | Trade history pulls.                                    |
| `/api/v5/account/balance`        | 5               | OKX guidance: 10 req / 2 s.                             |
| `/api/v5/trade/order`            | 30              | 60 requests / 2 seconds per‑instrument limit.           |
| `/api/v5/trade/orders-pending`   | 20              | Open order fetch.                                       |
| `/api/v5/trade/orders-history`   | 20              | Historical orders.                                      |
| `/api/v5/trade/fills`            | 30              | Execution reports.                                      |
| `/api/v5/trade/order-algo`       | 10              | Algo placements (conditional orders).                   |
| `/api/v5/trade/cancel-algos`     | 10              | Algo cancellation.                                      |

All keys automatically include the `okx:global` bucket. URLs are normalised (query strings removed) before rate limiting, so requests with different filters share the same quota.

:::info
For more details on rate limiting, see the official documentation: <https://www.okx.com/docs-v5/en/#rest-api-rate-limit>.
:::

## Configuration

### Configuration options

The OKX data client provides the following configuration options:

#### Data client

| Option                               | Default                         | Description |
|--------------------------------------|---------------------------------|-------------|
| `instrument_types`                   | `(OKXInstrumentType.SPOT,)`     | Controls which OKX instrument families are loaded (spot, swap, futures, options). |
| `contract_types`                     | `None`                          | Restricts loading to specific contract styles when combined with `instrument_types`. |
| `instrument_families`                | `None`                          | Instrument families to load (e.g., "BTC-USD", "ETH-USD"). Required for OPTIONS. Optional for FUTURES/SWAP. Not applicable for SPOT/MARGIN. |
| `base_url_http`                      | `None`                          | Override for the OKX REST endpoint; defaults to the production URL resolved at runtime. |
| `base_url_ws`                        | `None`                          | Override for the market data WebSocket endpoint. |
| `api_key`                            | `None`      | Falls back to `OKX_API_KEY` environment variable when unset. |
| `api_secret`                         | `None`      | Falls back to `OKX_API_SECRET` environment variable when unset. |
| `api_passphrase`                     | `None`      | Falls back to `OKX_API_PASSPHRASE` environment variable when unset. |
| `is_demo`                            | `False`                         | Connects to the OKX demo environment when `True`. |
| `http_timeout_secs`                  | `60`                            | Request timeout (seconds) for REST market data calls. |
| `max_retries`                        | `3`                             | Maximum retry attempts for recoverable REST errors. |
| `retry_delay_initial_ms`             | `1,000`                         | Initial delay (milliseconds) before retrying a failed request. |
| `retry_delay_max_ms`                 | `10,000`                        | Upper bound for exponential backoff delay between retries. |
| `update_instruments_interval_mins`   | `60`                            | Interval, in minutes, between background instrument refreshes. |
| `vip_level`                          | `None`                          | Enables higher‑depth order book channels when set to the matching OKX VIP tier. |
| `proxy_url`                          | `None`                          | Optional proxy URL for HTTP and WebSocket transports. |

The OKX execution client provides the following configuration options:

#### Execution client

| Option                     | Default     | Description |
|----------------------------|-------------|-------------|
| `instrument_types`         | `(OKXInstrumentType.SPOT,)` | Instrument families that should be tradable for this client. |
| `contract_types`           | `None`      | Restricts tradable contracts (linear, inverse, options) when paired with `instrument_types`. |
| `instrument_families`      | `None`      | Instrument families to load (e.g., "BTC-USD", "ETH-USD"). Required for OPTIONS. Optional for FUTURES/SWAP. Not applicable for SPOT/MARGIN. |
| `base_url_http`            | `None`      | Override for the OKX trading REST endpoint. |
| `base_url_ws`              | `None`      | Override for the private WebSocket endpoint. |
| `api_key`                  | `None`      | Falls back to `OKX_API_KEY` environment variable when unset. |
| `api_secret`               | `None`      | Falls back to `OKX_API_SECRET` environment variable when unset. |
| `api_passphrase`           | `None`      | Falls back to `OKX_API_PASSPHRASE` environment variable when unset. |
| `margin_mode`              | `None`      | Margin mode for derivatives trading (`ISOLATED` or `CROSS`). Only applies to SWAP/FUTURES/OPTIONS. Defaults to `ISOLATED` if not specified. |
| `use_spot_margin`          | `False`     | Enables margin/leverage for SPOT trading. When `True`, uses `isolated` or `cross` trade mode (determined by `margin_mode`). When `False`, uses `cash` trade mode (no leverage). Only applies to SPOT instruments. |
| `is_demo`                  | `False`     | Connects to the OKX demo trading environment. |
| `http_timeout_secs`        | `60`        | Request timeout (seconds) for REST trading calls. |
| `use_fills_channel`        | `False`     | Subscribes to the dedicated fills channel (VIP5+ required) for lower‑latency fill reports. |
| `use_mm_mass_cancel`       | `False`     | Uses the market‑maker bulk cancel endpoint when available; otherwise falls back to per‑order cancels. |
| `max_retries`              | `3`         | Maximum retry attempts for recoverable REST errors. |
| `retry_delay_initial_ms`   | `1,000`     | Initial delay (milliseconds) applied before retrying a failed request. |
| `retry_delay_max_ms`       | `10,000`    | Upper bound for the exponential backoff delay between retries. |
| `use_spot_cash_position_reports` | `False` | Generate position reports for SPOT CASH instruments based on wallet balances. |
| `proxy_url`                | `None`      | Optional proxy URL for HTTP and WebSocket transports. |

Below is an example configuration for a live trading node using OKX data and execution clients:

```python
from nautilus_trader.adapters.okx import OKX
from nautilus_trader.adapters.okx import OKXDataClientConfig, OKXExecClientConfig
from nautilus_trader.adapters.okx.factories import OKXLiveDataClientFactory, OKXLiveExecClientFactory
from nautilus_trader.config import InstrumentProviderConfig, TradingNodeConfig
from nautilus_trader.core.nautilus_pyo3 import OKXContractType
from nautilus_trader.core.nautilus_pyo3 import OKXInstrumentType
from nautilus_trader.core.nautilus_pyo3 import OKXMarginMode
from nautilus_trader.live.node import TradingNode

config = TradingNodeConfig(
    ...,
    data_clients={
        OKX: OKXDataClientConfig(
            api_key=None,           # Will use OKX_API_KEY env var
            api_secret=None,        # Will use OKX_API_SECRET env var
            api_passphrase=None,    # Will use OKX_API_PASSPHRASE env var
            base_url_http=None,
            instrument_provider=InstrumentProviderConfig(load_all=True),
            instrument_types=(OKXInstrumentType.SWAP,),
            contract_types=(OKXContractType.LINEAR,),
            is_demo=False,
        ),
    },
    exec_clients={
        OKX: OKXExecClientConfig(
            api_key=None,
            api_secret=None,
            api_passphrase=None,
            base_url_http=None,
            base_url_ws=None,
            instrument_provider=InstrumentProviderConfig(load_all=True),
            instrument_types=(OKXInstrumentType.SWAP,),
            contract_types=(OKXContractType.LINEAR,),
            is_demo=False,
        ),
    },
)
node = TradingNode(config=config)
node.add_data_client_factory(OKX, OKXLiveDataClientFactory)
node.add_exec_client_factory(OKX, OKXLiveExecClientFactory)
node.build()
```

## Contributing

:::info
For additional features or to contribute to the OKX adapter, please see our
[contributing guide](https://github.com/nautechsystems/nautilus_trader/blob/develop/CONTRIBUTING.md).
:::
