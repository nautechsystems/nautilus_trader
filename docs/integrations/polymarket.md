# Polymarket

Founded in 2020, Polymarket is a decentralized prediction market platform that enables
traders to speculate on event outcomes by buying and selling outcome tokens.

NautilusTrader provides a venue integration for data and execution via Polymarket's Central Limit
Order Book (CLOB) API.

Today the repository exposes two Polymarket implementations:

- The Python adapter in `nautilus_trader.adapters.polymarket`, which uses the
  [official Python CLOB V2 client library](https://github.com/Polymarket/py-clob-client-v2).
- The Rust-native adapter surface in `nautilus_trader.polymarket`, which NautilusTrader is
  consolidating toward.

:::warning
The two implementations overlap heavily, but they do not behave identically in every area.
This guide calls out the current differences where they matter.
:::

NautilusTrader supports multiple Polymarket signature types for order signing, which gives
flexibility for different wallet configurations while NautilusTrader handles signing and order
preparation.

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

A [binary option](https://en.wikipedia.org/wiki/Binary_option) is a type of financial exotic
option contract in which traders bet on the outcome of a yes-or-no proposition. If the
prediction is correct, the trader receives a fixed payout; otherwise, they receive nothing.
NautilusTrader represents Polymarket outcome tokens as `BinaryOption` instruments.

Polymarket uses **pUSD** as the collateral token for trading, [see below](#pusd) for more
information.

## Polymarket documentation

Polymarket offers resources for different audiences:

- [Polymarket Learn](https://learn.polymarket.com/): Educational content and guides for users
  to understand the platform and how to engage with it.
- [Polymarket CLOB API](https://docs.polymarket.com/trading/orders/overview): Technical
  documentation for developers interacting with the Polymarket CLOB API.

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
and won't need to work with these lower-level components directly.
:::

### Python and Rust implementations

The current docs cover both the Python adapter and the Rust-native adapter surface.
The table below shows the main differences that affect behavior today.

| Area                | Python adapter                                                                | Rust adapter                                                  | Notes |
|---------------------|-------------------------------------------------------------------------------|---------------------------------------------------------------|-------|
| Public package path | `nautilus_trader.adapters.polymarket`                                         | `nautilus_trader.polymarket`                                  | Rust is the consolidation target. |
| Order signing       | Uses `py-clob-client-v2`                                                      | Native Rust signing                                           | Python signing is slower. |
| Post‑only orders    | Supported for `GTC` and `GTD` only                                            | Supported for `GTC` and `GTD` only                            | Both reject post‑only with market TIF (`IOC` or `FOK`). |
| Batch submit        | Uses `POST /orders` for batchable `SubmitOrderList` requests                  | Uses `POST /orders` for batchable `SubmitOrderList` requests  | Both batch only independent limit orders, capped at 15 per request. |
| Batch cancel        | Uses `DELETE /orders`                                                         | Uses `DELETE /orders`                                         | Both align with official Polymarket docs. |
| Market unsubscribe  | Sends dynamic WebSocket `unsubscribe` messages                                | Sends dynamic WebSocket `unsubscribe` messages                | Both support subscribe and unsubscribe. |
| Data client config  | Credentials, subscription buffering, quote handling, provider config          | Base URLs, timeouts, filters, new‑market discovery            | Config surfaces differ materially. |
| Exec client config  | Credentials, retries, raw WS logging, experimental trade‑based order recovery | Credentials, retries, account IDs, native timeouts            | Rust does not expose every Python‑only option. |

## pUSD

**pUSD** is the collateral token used for trading on Polymarket. It is a standard ERC-20 token on
Polygon, backed by USDC.

The proxy contract address is
[0xC011a7E12a19f7B1f670d46F03B03f3342E82DFB](https://polygonscan.com/address/0xC011a7E12a19f7B1f670d46F03B03f3342E82DFB)
on Polygon. Direct on-chain funding wraps Polygon USDC.e (bridged USDC) into pUSD
through the [CollateralOnramp](https://docs.polymarket.com/resources/contracts).
The Bridge API can also deposit supported assets from other chains and credit pUSD
after conversion.

## Wallets and accounts

To interact with Polymarket via NautilusTrader, you'll need a **Polygon**-compatible wallet (such as MetaMask).

### Signature types

Polymarket supports multiple signature types for order signing and verification:

| Signature Type | Wallet Type                    | Description | Use Case |
|----------------|--------------------------------|-------------|----------|
| `0`            | EOA (Externally Owned Account) | Standard EIP712 signatures from wallets with direct private key control. | **Default.** Direct wallet connections (MetaMask, hardware wallets, etc.). |
| `1`            | Email/Magic Wallet Proxy       | Smart contract wallet for email‑based accounts (Magic Link). Only the email‑associated address can execute functions. | Polymarket Proxy associated with Email/Magic accounts. Requires `funder` address. |
| `2`            | Browser Wallet Proxy           | Modified Gnosis Safe (1-of-1 multisig) for browser wallets. | Polymarket Proxy associated with browser wallets. Enables UI verification. Requires `funder` address. |

:::note
See also: [Proxy wallet](https://docs.polymarket.com/developers/proxy-wallet) in the Polymarket documentation for more details about signature types and proxy wallet infrastructure.
:::

NautilusTrader defaults to signature type 0 (EOA) but can be configured to use any of the supported signature types via the `signature_type` configuration parameter.

A single wallet address is supported per trader instance when using environment variables,
or multiple wallets could be configured with multiple `PolymarketExecutionClient` instances.

:::note
Ensure your wallet is funded with **pUSD**, otherwise you will encounter the "not enough balance
or allowance" API error when submitting orders.
:::

### Setting allowances for Polymarket contracts

Before you can start trading, you need to ensure that your wallet has allowances set for Polymarket's smart contracts.
You can do this by running the provided script located at `nautilus_trader/adapters/polymarket/scripts/set_allowances.py`.

This script is adapted from a [gist](https://gist.github.com/poly-rodr/44313920481de58d5a3f6d1f8226bd5e) created by @poly-rodr.

:::note
You only need to run this script **once** per EOA wallet that you intend to use for trading on Polymarket.
:::

This script automates the process of approving the necessary allowances for the Polymarket contracts.
It sets approvals for the pUSD collateral token and Conditional Token Framework (CTF) contract to allow the
Polymarket CLOB Exchange to interact with your funds.

Before running the script, ensure the following prerequisites are met:

- Install the web3 Python package: `uv pip install "web3==7.12.1"`.
- Have a **Polygon**-compatible wallet funded with some POL (used for gas fees).
- Set the following environment variables in your shell:
  - `POLYGON_PRIVATE_KEY`: Your private key for the **Polygon**-compatible wallet.
  - `POLYGON_PUBLIC_KEY`: Your public key for the **Polygon**-compatible wallet.

Once you have these in place, the script will:

- Approve the maximum possible amount of pUSD (using the `MAX_INT` value) for the Polymarket collateral token contract.
- Set the approval for the CTF contract, allowing it to interact with your account for trading purposes.

:::note
You can also adjust the approval amount in the script instead of using `MAX_INT`,
with the amount specified in *fractional units* of **pUSD**, though this has not been tested.
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
- Signs and sends a transaction to approve the maximum pUSD allowance for Polymarket contracts.
- Sets approval for the CTF contract to manage Conditional Tokens on your behalf.
- Repeats the approval process for specific addresses like the Polymarket CLOB Exchange and Neg Risk adapter.

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
- `funder`: The **pUSD** funding wallet address used for funding trades. If not provided,
  will source the `POLYMARKET_FUNDER` environment variable.
- API credentials: You will need to provide the following API credentials to interact with the Polymarket CLOB:
  - `api_key`: If not provided, will source the `POLYMARKET_API_KEY` environment variable.
  - `api_secret`: If not provided, will source the `POLYMARKET_API_SECRET` environment variable.
  - `passphrase`: If not provided, will source the `POLYMARKET_PASSPHRASE` environment variable.
- `auto_load_missing_instruments` (default `True`): Controls whether subscribe and
  request commands for an instrument that is not already in the cache trigger an
  ad-hoc load via the Gamma API. When disabled, subscribing to an uncached
  instrument returns an error. See [Runtime instrument loading](#runtime-instrument-loading).
- `auto_load_debounce_ms` (default `100`): The window (milliseconds) over which
  concurrent auto-load requests are coalesced into a single batched Gamma call.

:::tip
We recommend using environment variables to manage your credentials.
:::

## Orders capability

Polymarket operates as a prediction market with a more limited set of order types and instructions compared to traditional exchanges.

### Order types

| Order Type             | Binary Options | Notes                                                                     |
|------------------------|----------------|---------------------------------------------------------------------------|
| `MARKET`               | ✓              | **BUY orders require quote quantity**, SELL orders require base quantity. |
| `LIMIT`                | ✓              |                                                                           |
| `STOP_MARKET`          | -              | *Not supported by Polymarket*.                                            |
| `STOP_LIMIT`           | -              | *Not supported by Polymarket*.                                            |
| `MARKET_IF_TOUCHED`    | -              | *Not supported by Polymarket*.                                            |
| `LIMIT_IF_TOUCHED`     | -              | *Not supported by Polymarket*.                                            |
| `TRAILING_STOP_MARKET` | -              | *Not supported by Polymarket*.                                            |

### Quantity semantics

Polymarket interprets order quantities differently depending on the order type *and* side:

- **Limit** orders interpret `quantity` as the number of conditional tokens (base units).
- **Market SELL** orders also use base-unit quantities.
- **Market BUY** orders interpret `quantity` as quote notional in **pUSD**.

As a result, a market buy order submitted with a base-denominated quantity will execute far
more size than intended.

When submitting market BUY orders, set `quote_quantity=True` on the order. The Python SDK
or Rust adapter converts the quote amount (pUSD) to the signed base-unit share amount before
posting to the CLOB. The Polymarket execution client denies base-denominated market buys to
prevent unintended fills.

```python
# Market BUY with quote quantity (spend $10 pUSD)
order = strategy.order_factory.market(
    instrument_id=instrument_id,
    order_side=OrderSide.BUY,
    quantity=instrument.make_qty(10.0),
    time_in_force=TimeInForce.IOC,  # Maps to Polymarket FAK
    quote_quantity=True,  # Interpret as pUSD notional
)
strategy.submit_order(order)
```

### Execution instructions

| Instruction   | Binary Options | Notes                                                |
|---------------|----------------|------------------------------------------------------|
| `post_only`   | ✓              | Supported for limit orders with `GTC` or `GTD` only. |
| `reduce_only` | -              | *Not supported by Polymarket*.                       |

### Time-in-force options

Polymarket calls the `POST /order` field `orderType`. In NautilusTrader, this maps to
`TimeInForce`. The valid combinations depend on the Nautilus order type:

| Nautilus TIF | Polymarket `orderType` | Nautilus order scope | Notes |
|--------------|------------------------|----------------------|-------|
| `GTC`        | `GTC`                  | `LIMIT` only         | Good‑Til‑Cancelled; rests on the book. |
| `GTD`        | `GTD`                  | `LIMIT` only         | Good‑Til‑Date; rests until expiration, fill, or cancel. |
| `FOK`        | `FOK`                  | `LIMIT` or `MARKET`  | Fill the full size immediately or cancel the whole order. |
| `IOC`        | `FAK`                  | `LIMIT` or `MARKET`  | Fill available size immediately and cancel the remainder. |

:::note
Polymarket uses `FAK` (Fill-And-Kill) for the semantics NautilusTrader calls
`IOC` (Immediate or Cancel). Polymarket docs classify `FOK` and `FAK` as market
order types, while `GTC` and `GTD` are limit order types. For Nautilus `MARKET`
orders, both adapters accept only `IOC` and `FOK`; `GTC` and `GTD` are valid for
resting `LIMIT` orders only.
:::

### Advanced order features

| Feature            | Binary Options | Notes                              |
|--------------------|----------------|------------------------------------|
| Order modification | -              | Cancellation functionality only.   |
| Bracket/OCO orders | -              | *Not supported by Polymarket.*     |
| Iceberg orders     | -              | *Not supported by Polymarket.*     |

### Batch operations

| Operation    | Binary Options | Notes                                                                                                                           |
|--------------|----------------|---------------------------------------------------------------------------------------------------------------------------------|
| Batch Submit | ✓              | Both adapters use `POST /orders` for independent limit‑order batches (max 15 orders per request). See [Batch submit](#batch-submit). |
| Batch Modify | -              | *Not supported by Polymarket*.                                                                                                  |
| Batch Cancel | ✓              | Both adapters use `DELETE /orders`.                                                                                             |

#### Batch submit

`SubmitOrderList` commands are routed to Polymarket's `POST /orders` endpoint. The endpoint
accepts at most 15 orders per request (`BATCH_ORDER_LIMIT`); larger lists are split into
sequential 15‑order chunks.

- Only `LIMIT` orders are batched. `MARKET` orders inside the list are routed to the
  single-order path, which signs a marketable order and submits it with `FAK` or `FOK`
  based on Nautilus `time_in_force`.
- `reduce_only` orders, `quote_quantity` orders, and `post_only` with market TIF
  (`IOC` or `FOK`) are rejected before submission.
- A single eligible order falls through to `POST /order` so it keeps the single‑order retry
  semantics; the batch path deliberately disables retry because the venue does not expose an
  idempotency key.
- `BatchCancelOrders` is dispatched to `DELETE /orders` in one shot.

### Submit error handling

Polymarket's public documentation describes successful
[`POST /order`](https://docs.polymarket.com/api-reference/trade/post-a-new-order) responses
with `success`, `orderID`, `status`, and `errorMsg`, and documents
[API errors](https://docs.polymarket.com/resources/error-codes) as structured error responses.
It does not document statusless `py-clob-client` exceptions or transport failures as
venue rejections.

The adapter rejects only when the response proves the order was not accepted, such as
`success=false`, a documented order processing error, or another non-retryable client/API
error. Transport failures, timeouts, ambiguous retry exhaustion, statusless `PolyApiException`,
malformed responses, and server-side failures keep the order submitted.

For unknown outcomes, both adapters derive the expected Polymarket order hash from the signed
EIP-712 order when possible and cache it as the `VenueOrderId`. Later WebSocket or reconciliation
reports then attach to the local `ClientOrderId` instead of becoming external orders.

Quote-quantity market BUY orders still apply the signed quote-to-base quantity update on the
unknown path. Cancels requested while submit outcome is unknown are deferred until the expected
venue order ID is known, and fill tracking is registered under that ID.

### Position management

| Feature          | Binary Options | Notes                             |
|------------------|----------------|-----------------------------------|
| Query positions  | ✓              | Current user positions from the Polymarket Data API. |
| Position mode    | -              | Binary outcome positions only.    |
| Leverage control | -              | No leverage available.            |
| Margin mode      | -              | No margin trading.                |

### Order querying

| Feature              | Binary Options | Notes                          |
|----------------------|----------------|--------------------------------|
| Query open orders    | ✓              | Active orders only.            |
| Query order history  | ✓              | Limited historical data.       |
| Order status updates | ✓              | Real‑time order state changes. |
| Trade history        | ✓              | Execution and fill reports.    |

### Contingent orders

| Feature            | Binary Options | Notes                               |
|--------------------|----------------|-------------------------------------|
| Order lists        | -              | Independent order batches exist, but linked contingency semantics do not. |
| OCO orders         | -              | *Not supported by Polymarket*.      |
| Bracket orders     | -              | *Not supported by Polymarket*.      |
| Conditional orders | -              | *Not supported by Polymarket*.      |

### Precision limits

Polymarket enforces different precision constraints based on tick size and `orderType`.

**Binary Option instruments** typically support up to 6 decimal places for amounts
(with 0.0001 tick size), but **market orders (`FAK` and `FOK`) have stricter
precision requirements**:

- **Market order types (`FAK` and `FOK`):**
  - Sell orders: maker amount limited to **2 decimal places**.
  - Taker amount: limited to **4 decimal places**.
  - The product `size × price` must not exceed **2 decimal places**.

- **Resting limit order types (`GTC` and `GTD`):** More flexible precision based on
  market tick size.

### Tick size precision hierarchy

| Tick Size | Price Decimals | Size Decimals | Amount Decimals |
|-----------|----------------|---------------|-----------------|
| 0.1       | 1              | 2             | 3               |
| 0.01      | 2              | 2             | 4               |
| 0.001     | 3              | 2             | 5               |
| 0.0001    | 4              | 2             | 6               |

:::note

- The tick size precision hierarchy is defined in the [`py-clob-client-v2` `ROUNDING_CONFIG`](https://github.com/Polymarket/py-clob-client-v2/blob/main/py_clob_client_v2/order_builder/builder.py).
- Market order precision limits (2 decimals for the size field, plus tick-derived bounds
  for the computed amount) come from the same `ROUNDING_CONFIG` and are enforced by
  `OrderBuilder.get_market_order_amounts` before signing.
- Tick sizes can change dynamically during market conditions, particularly when markets become one-sided.

:::

### Tick size change handling

When a market's tick size changes (`tick_size_change` WebSocket event), old
book levels can be invalid on the new grid (for example `0.505` fits a `0.001`
tick but not a `0.01` tick). To keep old-grid prices out of the new epoch, the
adapter treats the change as a book epoch transition:

1. Publish the updated `BinaryOption` with the new `price_increment` and `price_precision`.
2. Drop the local order book for the instrument.
3. Mark the instrument as awaiting a fresh snapshot.
4. Drop incremental `price_change` book deltas until the snapshot arrives.
5. Reseed the book from the snapshot and resume normal processing.

Trade ticks and the instrument update flow through unchanged. The Rust adapter
keeps emitting `QuoteTick` events through the gap by reading `best_bid` and
`best_ask` from each `price_change`. The Python adapter derives quotes from
the local book, so quote subscribers see the same brief gap as the deltas
(typically sub-second, until the venue snapshot arrives).

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

### Trade ID derivation

Polymarket does not publish a trade ID on `last_trade_price` market-data events.
The adapter derives a deterministic `TradeId` by FNV-1a hashing the asset ID,
side, price, size, and timestamp (`determine_trade_id` in both Rust and Python).
For CLOB Data API trade history the adapter composes the `TradeId` from a hash
suffix, an asset suffix, and a per-(transaction, asset) sequence number (format
`{transactionHash[-24:]}-{asset[-4:]}-{seq:06d}`). A single Polygon transaction
can settle multiple fills sharing the same `transactionHash`, so the older
last-36-character form collapsed those fills to a single id and downstream
catalogs silently dropped duplicates. The same venue event yields the same
trade ID across replays, keeping downstream dedup intact.

## Fees

Polymarket uses the formula `fee = C * feeRate * p * (1 - p)` where C is shares
traded and p is the share price. Fees peak at p = 0.50 and decrease symmetrically
toward the extremes. Only takers pay fees; makers pay zero.

| Category        | Taker `feeRate` | Maker `feeRate` | Maker rebate |
|-----------------|-----------------|-----------------|--------------|
| Crypto          | 0.072           | 0               | 20%          |
| Sports          | 0.03            | 0               | 25%          |
| Finance         | 0.04            | 0               | 25%          |
| Politics        | 0.04            | 0               | 25%          |
| Economics       | 0.05            | 0               | 25%          |
| Culture         | 0.05            | 0               | 25%          |
| Weather         | 0.05            | 0               | 25%          |
| Other / General | 0.05            | 0               | 25%          |
| Mentions        | 0.04            | 0               | 25%          |
| Tech            | 0.04            | 0               | 25%          |
| Geopolitics     | 0               | 0               | -            |

Fees are calculated in USDC, rounded to 5 decimal places, and applied at match time
by the protocol. The smallest fee charged is 0.00001 USDC; smaller fees round to zero.

:::note
For the latest rates, see Polymarket's [Fees](https://docs.polymarket.com/trading/fees) documentation.
:::

### Backtest fee model

For backtests, the adapter ships `PolymarketFeeModel` (a
`nautilus_trader.backtest.models.FeeModel` subclass) which applies the taker
fee formula above and credits passive maker fills with a rebate inferred from
the market category. Polymarket pays a 20% maker rebate on Crypto markets and
25% on other fee-enabled categories (Sports, Finance, Politics, Economics,
Culture, Weather, Tech, Mentions, Other), distributed daily from each market's
rebate pool. Geopolitics markets are fee-free with no rebates and the model
returns zero for them.

```python
from nautilus_trader.adapters.polymarket.fee_model import PolymarketFeeModel

# Default: maker rebates enabled
fee_model = PolymarketFeeModel()

# Or for taker-only strategies
fee_model = PolymarketFeeModel(maker_rebates_enabled=False)
```

The model can also be configured through `BacktestVenueConfig.fee_model` via
`ImportableFeeModelConfig` and `PolymarketFeeModelConfig`. Maker rebate share
inference uses the instrument's category labels first, then falls back to the
documented per-category fee rate when labels are absent.

## Reconciliation

The Polymarket API returns either all **active** (open) orders or specific orders when queried by the
Polymarket order ID (`venue_order_id`). The execution reconciliation procedure for Polymarket is as follows:

- Generate order reports for all instruments with active (open) orders, as reported by Polymarket.
- Generate position reports from current user positions reported by Polymarket's Data API.
- Compare these reports with Nautilus execution state.
- Generate missing orders to bring Nautilus execution state in line with positions reported by Polymarket.

**Note**: Polymarket does not directly provide data for orders which are no longer active.
The Python adapter exposes an experimental `generate_order_history_from_trades` option to fill some
of this gap from trade history. The Rust adapter does not expose the same option today.

:::warning
An optional execution client configuration, `generate_order_history_from_trades`, is currently under development.
It is not recommended for production use at this time.
:::

### Single-order recovery from trades

`/data/order/{id}` only returns active orders, so a `Filled` or `Canceled` order
returns an empty response. To avoid the engine resolving a local `ACCEPTED`
order as `REJECTED` (which discards fills that already happened at the venue),
`generate_order_status_report` falls back to `/data/trades` filtered by the
venue order ID. The cached order is resolved via `client_order_id`, falling
back to the cache's `venue_order_id` index when only the venue ID is known.
Recovery is keyed on the cached order; without one the recovery defers to the
engine rather than synthesizing an external order from trade history alone:

- Cached order + recovered fills covering the cached quantity (within
  `DUST_SNAP_THRESHOLD` for CLOB cent-tick truncation): returns `Filled`. The
  engine reconciles any delta over the cached `filled_qty` via inferred fill.
- Cached order + recovered fills that fall short of the cached quantity by
  more than dust: returns `Canceled` with the recovered `filled_qty`. The
  engine's CANCELED branch transitions the order at the cached `filled_qty`,
  so any newly recovered fills that arrived only via REST (not WS) are not
  applied in this rare partial-cancel case. Closing the order is preferred
  over leaving it stuck open; if exact fill metadata matters in this scenario
  the venue trade history can be reviewed manually.
- Cached order, no trades: returns `Canceled` with
  `cancel_reason="ORDER_NOT_FOUND_AT_VENUE"`.
- No cached order (regardless of trades): returns `None`; the engine's
  not-found-at-venue path resolves the local entry.

`open_check_interval_secs` is recommended for Polymarket so the engine
periodically drives this recovery path for orders whose terminal WS update
was missed.

## Fill quantity normalization

Polymarket reports fill quantities that drift slightly from the submitted
order quantity due to protocol-level rounding: the CLOB rounds matched fills
to integer cent ticks (underfill) and the V2 SDK truncates `takerAmount` to
USDC scale on market-BUY quote-quantity orders (overfill, a few microshares).
Both drift sources are fixed in absolute share terms, so the adapter
normalizes them with a single threshold of `DUST_SNAP_THRESHOLD = 0.01`
shares. Anything beyond that surfaces to the engine as a real partial fill or
overfill.

| Direction | Source                                  | Adapter behaviour                         |
|-----------|-----------------------------------------|-------------------------------------------|
| Overfill  | V2 USDC‑scale truncation (microshares)  | Snap fill DOWN to `submitted_qty`         |
| Underfill | CLOB cent‑tick truncation (≤ `0.01`)    | Preserved; synthetic dust fill at MATCHED |

`FillReport.commission` always reflects the venue-reported size, not the
snapped quantity. The few-ulp difference is sub-microcent in pUSD.

The fill tracker is keyed by `venue_order_id` and registered on order
accept, so fill reports for orders placed in another session pass through
unchanged. `DUST_SNAP_THRESHOLD` is not configurable per-strategy; it lives
in `nautilus_polymarket::common::consts`.

## WebSockets

The `PolymarketWebSocketClient` is built on top of the high-performance Nautilus `WebSocketClient` base class, written in Rust.

### Data

The data adapter buffers the initial `market` subscriptions during the connection window and then
subscribes dynamically as new instruments are requested.
The client manages multiple WebSocket connections internally when the subscription count grows past
the configured per-connection cap.

### Runtime instrument loading

Polymarket lists thousands of active markets and new markets appear throughout the day, so preloading
the full universe at startup is rarely practical. The data adapter auto-loads missing instruments on
demand so that strategies can subscribe to markets that are not in the cache:

- When a strategy issues `subscribe_quote_ticks`, `subscribe_trade_ticks`, `subscribe_order_book_deltas`,
  or `request_instrument` for an instrument that is not cached, the adapter registers the request and
  waits `auto_load_debounce_ms` (default 100 ms) so that concurrent requests coalesce.
- It then issues a single batched Gamma API call. Batches larger than the Gamma `condition_ids`
  query ceiling (about 100) are split across multiple calls and merged.
- Once the instruments are loaded, they are published to the data engine (populating the cache)
  and the deferred subscriptions open their WebSocket subscriptions atomically. A strategy that
  unsubscribes while the auto-load is in flight does not see a spurious subscription opened.

The feature is enabled by default. Disable it by setting `auto_load_missing_instruments=False` on
`PolymarketDataClientConfig`. To preload a known set of markets at startup instead, supply
`load_ids` or `event_slug_builder` on `PolymarketInstrumentProviderConfig`.

### Purging instruments at runtime

Polymarket auto-loads instruments on demand, so a long-running session keeps growing the cache as
markets resolve, new markets appear, and strategies cycle through events. Use `cache.purge_instrument`
to drop markets the strategy no longer tracks. The call removes the instrument record and every
cache-owned map keyed by it (order book, quotes, trades, bars).

```python
class PolymarketHousekeeping(Strategy):
    def on_position_closed(self, event: PositionClosed) -> None:
        # Drop the market once the position is closed and you have no further interest.
        instrument_id = event.instrument_id
        self.unsubscribe_quote_ticks(instrument_id)
        self.unsubscribe_order_book_deltas(instrument_id)
        self.cache.purge_instrument(instrument_id)
```

Common triggers on Polymarket:

- A market resolves and produces no further trades.
- An event ends and the strategy rotates off its markets.
- The strategy rotates a fixed-size watchlist and drops the oldest entry.

The purge skips any instrument that still has non-terminal orders (initialized, submitted,
accepted, emulated, released, or inflight) or non-closed positions, so it is safe to call without
coordinating with the execution client. Active WebSocket subscriptions belong to the data engine.
Unsubscribe before purging if you no longer want updates.

The cache also exposes `purge_order`, `purge_position`, `purge_closed_orders`,
`purge_closed_positions`, and `purge_account_events` for trimming closed execution state.
For long-running Polymarket nodes, schedule the bulk purges from `LiveExecEngineConfig`
(15 min interval, 60 min buffer is a sensible default). See
[Cache: purging cached data](../concepts/cache.md#purging-cached-data) for the full set.

:::warning
The caller decides when an instrument is no longer needed. Purging an instrument that another
actor, strategy, or engine still relies on causes missing instrument lookups and loses market-data
history.
:::

### Execution

The execution adapter keeps a `user` channel connection for order and trade events and manages market
subscriptions as needed for instruments seen during trading.

Both the Python and Rust adapters support dynamic WebSocket subscribe and unsubscribe operations.

### Subscription limits

Polymarket enforces a **maximum of 500 instruments per WebSocket connection** (undocumented limitation).

When you attempt to subscribe to 501 or more instruments on a single WebSocket connection:

- You will **not** receive the initial order book snapshot for each instrument.
- You will only receive subsequent order book updates.

NautilusTrader automatically manages WebSocket connections to handle this limitation:

- The adapter defaults to **200 instrument subscriptions per connection** (configurable via `ws_max_subscriptions_per_connection`).
- When the subscription count exceeds this limit, additional WebSocket connections are created automatically.
- This ensures you receive complete order book data (including initial snapshots) for all subscribed instruments.

:::tip
If you need to subscribe to a large number of instruments (e.g., 5000+), the adapter will automatically distribute these subscriptions across multiple WebSocket connections.
You can tune the per-connection limit up to 500 via `ws_max_subscriptions_per_connection`.
:::

## Rate limiting

Polymarket enforces rate limits via Cloudflare throttling.
When limits are exceeded, requests are throttled on sliding windows. Sustained
overshoot can still surface as HTTP 429 responses or temporary blocking.

### REST limits

Polymarket changes these quotas over time. As of 2026-05-06, the official limits are:

| Endpoint                      | Burst (10s) | Sustained (10 min) | Notes |
|-------------------------------|-------------|--------------------|-------|
| General rate limiting         | 15,000      | -                  | Global documented rate limit. |
| Health check (`/ok`)          | 100         | -                  | Health endpoint. |
| CLOB general                  | 9,000       | -                  | Aggregate across CLOB endpoints. |
| CLOB `POST /order`            | 3,500       | 36,000             | Single‑order submit. |
| CLOB `POST /orders`           | 1,000       | 15,000             | Batch submit (up to 15 orders per request). |
| CLOB `DELETE /order`          | 3,000       | 30,000             | Single‑order cancel. |
| CLOB `DELETE /orders`         | 1,000       | 15,000             | Batch cancel. |
| CLOB `GET /balance-allowance` | 200         | -                  | Balance and allowance queries. |
| CLOB API key endpoints        | 100         | -                  | Key management. |
| Gamma general                 | 4,000       | -                  | Aggregate across Gamma endpoints. |
| Gamma `/markets`              | 300         | -                  | Market metadata. |
| Gamma `/events`               | 500         | -                  | Event metadata. |
| Data general                  | 1,000       | -                  | Aggregate across Data API endpoints. |
| Data `/trades`                | 200         | -                  | Trade history. |
| Data `/positions`             | 150         | -                  | Current positions. |

### WebSocket limits

The WebSocket quotas are not part of the published REST rate-limits table.
The adapter ships a configurable per-connection subscription cap
(`ws_max_subscriptions_per_connection`) defaulting to 200; Polymarket previously
documented an upper bound of 500 per connection.

:::warning
Exceeding Polymarket rate limits triggers Cloudflare throttling. Requests are queued
using sliding windows rather than rejected immediately, but sustained overshoot can
result in HTTP 429 responses or temporary blocking.
:::

### Data loader rate limiting

The `PolymarketDataLoader` includes built-in rate limiting when using the default HTTP client.
Requests are automatically throttled to 100 requests per minute by default.
That is a NautilusTrader default, not Polymarket's current published limit.
The current Rust HTTP clients also ship with conservative 100 requests per minute quotas.

When fetching large date ranges across multiple markets:

- Multiple loaders sharing the same `http_client` instance will coordinate rate limiting automatically.
- For higher throughput, pass a custom `http_client` with adjusted quotas.
- The loader does not implement automatic retry on 429 errors, so implement backoff if needed.

:::info
For the latest rate limit details, see the official Polymarket documentation:
<https://docs.polymarket.com/api-reference/rate-limits>
:::

## Limitations and considerations

The following limitations are currently known:

- Python order signing via `py-clob-client-v2` is slow and can take around one second per order.
- Reduce-only orders are not supported.
- Batch submit (`POST /orders`) accepts at most 15 orders per request; the adapter splits larger `SubmitOrderList` commands into sequential 15-order chunks.

## Configuration

The Python adapter (`nautilus_trader.adapters.polymarket`) and the Rust-native adapter
(`nautilus_trader.polymarket`) expose different config surfaces. The tables below document
both adapters in full.

### Data client options (Python v2)

Class: `PolymarketDataClientConfig` in `nautilus_trader.adapters.polymarket.config`.

| Option                                | Default      | Description |
|---------------------------------------|--------------|-------------|
| `venue`                               | `POLYMARKET` | Venue identifier registered for the data client. |
| `private_key`                         | `None`       | Wallet private key; sourced from `POLYMARKET_PK` when omitted. |
| `signature_type`                      | `0`          | Signature scheme (0 = EOA, 1 = email proxy, 2 = browser wallet proxy). |
| `funder`                              | `None`       | pUSD funding wallet; sourced from `POLYMARKET_FUNDER` when omitted. |
| `api_key`                             | `None`       | API key; sourced from `POLYMARKET_API_KEY` when omitted. |
| `api_secret`                          | `None`       | API secret; sourced from `POLYMARKET_API_SECRET` when omitted. |
| `passphrase`                          | `None`       | API passphrase; sourced from `POLYMARKET_PASSPHRASE` when omitted. |
| `base_url_http`                       | `None`       | Override for the REST base URL. |
| `base_url_ws`                         | `None`       | Override for the WebSocket base URL. |
| `proxy_url`                           | `None`       | Optional proxy URL for HTTP and WebSocket transports. |
| `ws_connection_initial_delay_secs`    | `5`          | Delay (seconds) before the first WebSocket connection to buffer subscriptions. |
| `ws_connection_delay_secs`            | `0.1`        | Delay (seconds) between subsequent WebSocket connection attempts. |
| `ws_max_subscriptions_per_connection` | `200`        | Maximum instrument subscriptions per WebSocket connection (Polymarket limit is 500). |
| `update_instruments_interval_mins`    | `60`         | Interval (minutes) between instrument catalogue refreshes. |
| `compute_effective_deltas`            | `False`      | Compute effective order book deltas for bandwidth savings. |
| `drop_quotes_missing_side`            | `True`       | Drop quotes with missing bid/ask prices instead of substituting boundary values. |
| `auto_load_missing_instruments`       | `True`       | Load instruments on demand when subscribe or request commands reference uncached instruments. |
| `auto_load_debounce_ms`               | `100`        | Debounce window (milliseconds) for coalescing concurrent runtime instrument loads. |
| `instrument_config`                   | `None`       | Optional `PolymarketInstrumentProviderConfig` for instrument loading. |

### Execution client options (Python v2)

Class: `PolymarketExecClientConfig` in `nautilus_trader.adapters.polymarket.config`.

| Option                                | Default      | Description |
|---------------------------------------|--------------|-------------|
| `venue`                               | `POLYMARKET` | Venue identifier registered for the execution client. |
| `private_key`                         | `None`       | Wallet private key; sourced from `POLYMARKET_PK` when omitted. |
| `signature_type`                      | `0`          | Signature scheme (0 = EOA, 1 = email proxy, 2 = browser wallet proxy). |
| `funder`                              | `None`       | pUSD funding wallet; sourced from `POLYMARKET_FUNDER` when omitted. |
| `api_key`                             | `None`       | API key; sourced from `POLYMARKET_API_KEY` when omitted. |
| `api_secret`                          | `None`       | API secret; sourced from `POLYMARKET_API_SECRET` when omitted. |
| `passphrase`                          | `None`       | API passphrase; sourced from `POLYMARKET_PASSPHRASE` when omitted. |
| `base_url_http`                       | `None`       | Override for the REST base URL. |
| `base_url_ws`                         | `None`       | Override for the WebSocket base URL. |
| `base_url_data_api`                   | `None`       | Override for the Data API base URL (default `https://data-api.polymarket.com`). |
| `proxy_url`                           | `None`       | Optional proxy URL for HTTP and WebSocket transports. |
| `ws_max_subscriptions_per_connection` | `200`        | Maximum instrument subscriptions per WebSocket connection (Polymarket limit is 500). |
| `max_retries`                         | `None`       | Maximum retry attempts for submit/cancel requests. |
| `retry_delay_initial_ms`              | `None`       | Initial delay (milliseconds) between retries. |
| `retry_delay_max_ms`                  | `None`       | Maximum delay (milliseconds) between retries. |
| `ack_timeout_secs`                    | `5.0`        | Timeout (seconds) to wait for order/trade acknowledgment from cache. |
| `generate_order_history_from_trades`  | `False`      | Generate synthetic order history from trade reports when `True` (experimental). |
| `log_raw_ws_messages`                 | `False`      | Log raw WebSocket payloads at INFO level when `True`. |
| `instrument_config`                   | `None`       | Optional `PolymarketInstrumentProviderConfig` for instrument loading. |

### Data client options (Rust v2)

Struct: `PolymarketDataClientConfig` in `crates/adapters/polymarket/src/config.rs`.

| Option                             | Default                                    | Description |
|------------------------------------|--------------------------------------------|-------------|
| `base_url_http`                    | `None` (official CLOB endpoint)            | Override for the CLOB REST base URL. |
| `base_url_ws`                      | `None` (official CLOB endpoint)            | Override for the CLOB WebSocket base URL. |
| `base_url_gamma`                   | `None` (official Gamma endpoint)           | Override for the Gamma API base URL. |
| `base_url_data_api`                | `None` (`https://data-api.polymarket.com`) | Override for the Data API base URL. |
| `http_timeout_secs`                | `60`                                       | HTTP request timeout (seconds). |
| `ws_timeout_secs`                  | `30`                                       | WebSocket connect/idle timeout (seconds). |
| `ws_max_subscriptions`             | `200`                                      | Maximum instrument subscriptions per WebSocket connection. |
| `update_instruments_interval_mins` | `60`                                       | Interval (minutes) between instrument catalogue refreshes. |
| `subscribe_new_markets`            | `false`                                    | Subscribe to new‑market discovery events via WebSocket when `true`. |
| `auto_load_missing_instruments`    | `true`                                     | Load instruments on demand when subscribe or request commands reference uncached instruments. |
| `auto_load_debounce_ms`            | `100`                                      | Debounce window (milliseconds) for coalescing concurrent runtime instrument loads. |
| `filters`                          | `[]`                                       | Instrument filters applied during loading and discovery. |
| `new_market_filter`                | `None`                                     | Optional filter applied to newly discovered markets before emission. |
| `transport_backend`                | `Sockudo`                                  | WebSocket transport backend. |

The Rust data client config does not accept account credentials; authentication is handled by
the execution client. Subscription buffering (`ws_connection_initial_delay_secs`) and quote
handling (`compute_effective_deltas`, `drop_quotes_missing_side`) are Python-only today.

### Execution client options (Rust v2)

Struct: `PolymarketExecClientConfig` in `crates/adapters/polymarket/src/config.rs`.

| Option                   | Default                                    | Description |
|--------------------------|--------------------------------------------|-------------|
| `trader_id`              | default `TraderId`                         | Trader identifier the client registers under. |
| `account_id`             | `POLYMARKET-001`                           | Account identifier for this execution client. |
| `private_key`            | `None` (`POLYMARKET_PK` env)               | Wallet private key for EIP-712 signing. |
| `api_key`                | `None` (`POLYMARKET_API_KEY` env)          | CLOB API key (L2 auth). |
| `api_secret`             | `None` (`POLYMARKET_API_SECRET` env)       | CLOB API secret (L2 auth). |
| `passphrase`             | `None` (`POLYMARKET_PASSPHRASE` env)       | CLOB API passphrase (L2 auth). |
| `funder`                 | `None` (`POLYMARKET_FUNDER` env)           | pUSD funding wallet. |
| `signature_type`         | `Eoa`                                      | Signature scheme (`Eoa`, `PolyProxy`, `PolyGnosisSafe`). |
| `base_url_http`          | `None` (official CLOB endpoint)            | Override for the CLOB REST base URL. |
| `base_url_ws`            | `None` (official CLOB endpoint)            | Override for the CLOB WebSocket base URL. |
| `base_url_data_api`      | `None` (`https://data-api.polymarket.com`) | Override for the Data API base URL. |
| `http_timeout_secs`      | `60`                                       | HTTP request timeout (seconds). |
| `max_retries`            | `3`                                        | Maximum retry attempts for single‑order submit/cancel requests. |
| `retry_delay_initial_ms` | `1000`                                     | Initial delay (milliseconds) between retries. |
| `retry_delay_max_ms`     | `10000`                                    | Maximum delay (milliseconds) between retries. |
| `ack_timeout_secs`       | `5`                                        | Timeout (seconds) waiting for WebSocket order/trade acknowledgment. |
| `transport_backend`      | `Sockudo`                                  | WebSocket transport backend. |

The Rust execution client does not expose `generate_order_history_from_trades`,
`log_raw_ws_messages`, `ws_max_subscriptions_per_connection`, or `instrument_config`. Batch
submissions via `POST /orders` deliberately skip retry regardless of `max_retries`; the
single-order path still retries on transient failures.

### Instrument provider configuration options

The instrument provider config is passed via the `instrument_config` parameter on the data client config.

| Option               | Default | Description                                                                                    |
|----------------------|---------|------------------------------------------------------------------------------------------------|
| `load_all`           | `False` | Load all venue instruments on start. Auto‑set to `True` when `event_slug_builder` is provided. |
| `event_slug_builder` | `None`  | Fully qualified path to a callable returning event slugs (e.g., `"mymodule:build_slugs"`).     |

#### Event slug builder

The `event_slug_builder` feature enables efficient loading of niche markets without downloading
the full venue catalogue. Instead of loading everything, you provide a function that returns
event slugs for the specific markets you need.

```python
from nautilus_trader.adapters.polymarket.providers import PolymarketInstrumentProviderConfig

# Configure with a slug builder function
instrument_config = PolymarketInstrumentProviderConfig(
    event_slug_builder="myproject.slugs:build_temperature_slugs",
)
```

The callable must have signature `() -> list[str]` and return a list of event slugs:

```python
# myproject/slugs.py
from datetime import UTC, datetime, timedelta

def build_temperature_slugs() -> list[str]:
    """Build slugs for NYC temperature markets."""
    slugs = []
    today = datetime.now(tz=UTC).date()

    for i in range(7):
        date = today + timedelta(days=i)
        slug = f"highest-temperature-in-nyc-on-{date.strftime('%B-%d').lower()}"
        slugs.append(slug)

    return slugs
```

See `examples/live/polymarket/slug_builders.py` for more examples including crypto UpDown markets.

## Historical data loading

The `PolymarketDataLoader` provides methods for fetching and parsing historical market data
for research and backtesting purposes. The loader integrates with multiple Polymarket APIs to provide the required data.

:::note
All data fetching methods are **asynchronous** and must be called with `await`. The loader can optionally accept an `http_client` parameter for dependency injection (useful for testing).
:::

### Data sources

The loader fetches data from three primary sources:

1. **Polymarket Gamma API** - Market metadata, instrument details, and active market listings.
2. **Polymarket CLOB API** - Market details for instrument construction.
3. **Polymarket Data API** - Historical trades and current user positions.

The current loader does **not** expose helpers for CLOB price history timeseries or order book
history snapshots.

### Method naming conventions

The loader provides two ways to access the Polymarket APIs:

| Prefix    | Type             | Use case                                                               |
|-----------|------------------|------------------------------------------------------------------------|
| `query_*` | Static methods   | API exploration without an instrument. No loader instance needed.      |
| `fetch_*` | Instance methods | Data fetching with a configured loader. Uses the loader's HTTP client. |

**Use `query_*` when** you want to explore markets, discover events, or fetch metadata
before committing to a specific instrument:

```python
# No loader needed: query the API directly
market = await PolymarketDataLoader.query_market_by_slug("some-market")
event = await PolymarketDataLoader.query_event_by_slug("some-event")
```

**Use `fetch_*` when** you have a loader instance and want to fetch data using its
configured HTTP client (for coordinated rate limiting across multiple calls):

```python
loader = await PolymarketDataLoader.from_market_slug("some-market")

# All fetch calls share the loader's HTTP client
markets = await loader.fetch_markets(active=True, limit=100)
events = await loader.fetch_events(active=True)
details = await loader.fetch_market_details(condition_id)
```

### Finding markets

Use the provided utility scripts to discover active markets:

```bash
# List all active markets
python nautilus_trader/adapters/polymarket/scripts/active_markets.py

# List BTC and ETH UpDown markets specifically
python nautilus_trader/adapters/polymarket/scripts/list_updown_markets.py
```

### Basic usage

The recommended way to create a loader is using the factory classmethods, which handle
all the API calls and instrument creation automatically:

```python
import asyncio

from nautilus_trader.adapters.polymarket import PolymarketDataLoader

async def main():
    # Create loader from market slug (recommended)
    loader = await PolymarketDataLoader.from_market_slug("gta-vi-released-before-june-2026")

    # Loader is ready to use with instrument and token_id set
    print(loader.instrument)
    print(loader.token_id)

asyncio.run(main())
```

For events with multiple markets (e.g., temperature buckets), use `from_event_slug`:

```python
# Returns a list of loaders, one per market in the event
loaders = await PolymarketDataLoader.from_event_slug("highest-temperature-in-nyc-on-january-26")
```

#### Look-ahead protection for resolved markets

When constructing a loader for a market that has already resolved at backtest
build time, the venue payload includes the answer (`closed`, `closedTime`,
`umaResolutionStatus`, per-token `winner`). A strategy that reads
`cache.instrument(...).info` from `on_start` can therefore see the outcome
before the simulation runs.

Pass `sanitize_info=True` to either factory to redact those fields from
`instrument.info` before the instrument is constructed. The redacted slice is
stashed on the loader as `resolution_metadata` for post-hoc analytics
(settlement PnL, Brier scoring) without leaking it into the simulation:

```python
loader = await PolymarketDataLoader.from_market_slug(
    "some-resolved-market",
    sanitize_info=True,
)

assert "closed" not in loader.instrument.info
assert loader.resolution_metadata["closed"] is True
```

### Discovering markets and events

Use `fetch_markets()` and `fetch_events()` to discover available markets programmatically:

```python
loader = await PolymarketDataLoader.from_market_slug("any-market")

# List active markets
markets = await loader.fetch_markets(active=True, closed=False, limit=100)
for market in markets:
    print(f"{market['slug']}: {market['question']}")

# List active events
events = await loader.fetch_events(active=True, limit=50)
for event in events:
    print(f"{event['slug']}: {event['title']}")

# Get all markets within a specific event
event_markets = await loader.get_event_markets("highest-temperature-in-nyc-on-january-26")
```

For quick exploration without creating a loader, use the static `query_*` methods
(see [Method naming conventions](#method-naming-conventions) above).

### Fetching trade history

The `load_trades()` convenience method fetches and parses historical trades in one step:

```python
import pandas as pd

# Load all available trades
trades = await loader.load_trades()

# Or filter by time range (client-side filtering)
end = pd.Timestamp.now(tz="UTC")
start = end - pd.Timedelta(hours=24)

trades = await loader.load_trades(
    start=start,
    end=end,
)
```

Alternatively, you can fetch and parse separately using the lower-level methods:

```python
condition_id = loader.condition_id

# Fetch raw trades from the Polymarket Data API
raw_trades = await loader.fetch_trades(condition_id=condition_id)

# Parse to NautilusTrader TradeTicks
trades = loader.parse_trades(raw_trades)
```

Trade data is sourced from the [Polymarket Data API](https://data-api.polymarket.com/trades),
which provides real execution data including price, size, side, and on-chain transaction hash.

:::note
The public Data API caps offset-based pagination on high-activity markets. When
this ceiling is hit the loader emits a `RuntimeWarning` and returns the trades
fetched up to the cap rather than aborting the load. Use another historical
data source if you need full coverage of a heavily traded market.
:::

### Complete backtest example

See `examples/backtest/polymarket_simple_quoter.py` for a full example:

```python
import asyncio
from decimal import Decimal

from nautilus_trader.adapters.polymarket import POLYMARKET_VENUE
from nautilus_trader.adapters.polymarket import PolymarketDataLoader
from nautilus_trader.backtest.config import BacktestEngineConfig
from nautilus_trader.backtest.engine import BacktestEngine
from nautilus_trader.examples.strategies.ema_cross_long_only import EMACrossLongOnly
from nautilus_trader.examples.strategies.ema_cross_long_only import EMACrossLongOnlyConfig
from nautilus_trader.model.currencies import pUSD
from nautilus_trader.model.data import BarType
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import OmsType
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.model.objects import Money

async def run_backtest():
    # Initialize loader and fetch market data
    loader = await PolymarketDataLoader.from_market_slug("gta-vi-released-before-june-2026")
    instrument = loader.instrument

    # Load historical trades from the Polymarket Data API
    trades = await loader.load_trades()

    # Configure and run backtest
    config = BacktestEngineConfig(trader_id=TraderId("BACKTESTER-001"))
    engine = BacktestEngine(config=config)

    engine.add_venue(
        venue=POLYMARKET_VENUE,
        oms_type=OmsType.NETTING,
        account_type=AccountType.CASH,
        base_currency=pUSD,
        starting_balances=[Money(10_000, pUSD)],
    )

    engine.add_instrument(instrument)
    engine.add_data(trades)

    bar_type = BarType.from_str(f"{instrument.id}-100-TICK-LAST-INTERNAL")
    strategy_config = EMACrossLongOnlyConfig(
        instrument_id=instrument.id,
        bar_type=bar_type,
        trade_size=Decimal("20"),
    )

    strategy = EMACrossLongOnly(config=strategy_config)
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
    condition_id="0xcccb7e7613a087c132b69cbf3a02bece3fdcb824c1da54ae79acc8d4a562d902",
    token_id="8441400852834915183759801017793514978104486628517653995211751018945988243154"
)
```

## Contributing

:::info
For additional features or to contribute to the Polymarket adapter, please see our
[contributing guide](https://github.com/nautechsystems/nautilus_trader/blob/develop/CONTRIBUTING.md).
:::
