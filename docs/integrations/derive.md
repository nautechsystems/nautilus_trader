# Derive

Derive (formerly Lyra) is a decentralized derivatives venue offering European-style options
and cash-settled perpetual swaps, and one of the largest on-chain options markets. Trading
runs against a per-user smart-contract wallet on the Derive Chain, so collateral stays in the
user's custody while orders match through the venue's orderbook.

The Derive Chain is an optimistic rollup that settles to Ethereum. Orders match off chain and
settle on chain, pairing orderbook execution with self-custody. Orders are authorized with
EIP-712 typed-data signatures from a session key scoped to a subaccount, which keeps the
signing key separate from the wallet owner and lets users rotate or revoke access without
moving funds.

## Examples

Rust example testers live in
[`crates/adapters/derive/examples/`](https://github.com/nautechsystems/nautilus_trader/tree/develop/crates/adapters/derive/examples/).

## Overview

The Derive adapter is implemented in Rust under `crates/adapters/derive`. It exposes:

- `DeriveHttpClient`: Low-level REST connectivity to `api.lyra.finance` (mainnet) or `api-demo.lyra.finance` (testnet).
- `DeriveWebSocketClient`: JSON-RPC WebSocket transport with subscription tracking, reconnect, and signed order entry (the WebSocket Trading API).
- `DeriveInstrumentProvider`: Per-currency instrument fetch and caching.
- `DeriveDataClient`: Live market data client.
- `DeriveDataClientFactory`: Data client factory for the live node builder.
- `DeriveExecutionClient`: Live execution client for signed order, cancel, query, and report flows.
- `DeriveExecutionClientFactory`: Execution client factory for the live node builder.

Execution flows use EIP-712 typed-data signing against the Derive Chain per-action
module contracts.

## Derive documentation

Derive publishes API documentation at [docs.derive.xyz](https://docs.derive.xyz). Refer to it
alongside this guide for additional details.

## Products

| Product type           | Supported | Notes                                                                |
|------------------------|-----------|----------------------------------------------------------------------|
| ERC-20 spot            | ✓         | USDC‑quoted pairs such as `ETH-USDC`; parsed as `CurrencyPair`.      |
| Perpetual swaps        | ✓         | Cash‑settled in USDC, with per‑currency listings such as `ETH-PERP`. |
| Options (calls / puts) | ✓         | European‑style options using `{CURRENCY}-{EXPIRY}-{STRIKE}-{C|P}`.   |

## Symbology

Derive instruments use the native venue symbol with the venue suffix `.DERIVE`:

- Spot: `ETH-USDC.DERIVE` (base currency, quote currency).
- Perpetual: `ETH-PERP.DERIVE`, `BTC-PERP.DERIVE`.
- Option: `ETH-20260626-3000-C.DERIVE` (currency, expiry, strike, kind).

The first hyphen-separated segment of the symbol is the underlying currency. The provider
fetches `public/get_instruments` once per currency, so subscribing to a new currency triggers a
lazy REST fetch when `auto_load_missing_instruments` is enabled (the default).

The adapter routes on the venue `instrument_type` (`perp`, `option`, `erc20`), not on the symbol
suffix, so spot pairs need no special symbology parsing. Spot reuses the same Trade-module
signing path as perps and options; the in-repo fixtures under
`crates/adapters/derive/test_data/spot/` capture the spot instrument, order book, ticker, and
trade field shapes the parser and execution paths are pinned to.

:::warning
Spot trading has had less live exercise than perpetuals and options. Testnet accepts and cancels a
passive `ETH-USDC` limit order at the `0.1 ETH` minimum amount, and mainnet place/cancel has been
exercised manually. Public spot trade channels (`trades.erc20.ETH`, `trades.ETH-USDC`) subscribe
successfully but can be low-volume, so expect sparse trade frames.
:::

## Environments

Configure the environment with the `DeriveEnvironment` enum on either client config.

| Environment | Config                       | REST                            | WebSocket                        |
|-------------|------------------------------|---------------------------------|----------------------------------|
| Mainnet     | `DeriveEnvironment::Mainnet` | `https://api.lyra.finance`      | `wss://api.lyra.finance/ws`      |
| Testnet     | `DeriveEnvironment::Testnet` | `https://api-demo.lyra.finance` | `wss://api-demo.lyra.finance/ws` |

Testnet is a separate chain with its own session keys and balances; mainnet and testnet API
keys are not interchangeable. Public market data (book, ticker, trades) does not require
credentials.

EIP-712 Protocol Constants (`DOMAIN_SEPARATOR`, `ACTION_TYPEHASH`, per-action module
addresses) for both networks are shipped in `crates/adapters/derive/src/common/consts.rs`
and tracked against Derive's [Protocol Constants reference](https://docs.derive.xyz/reference/protocol-constants).
`DeriveExecClientConfig::domain_separator`, `action_typehash`, and `trade_module_address`
accept per-instance overrides that take precedence over the shipped values.

## Testnet onboarding

Derive labels the demo environment "testnet" in the web app and "demo" in the API hostname.
This guide uses "testnet" to match the dashboard and our `DeriveEnvironment::Testnet` enum.
Steps to reach a position where the execution client can submit a signed order:

1. **Sign in to the testnet dashboard.** Open
   [testnet.derive.xyz](https://testnet.derive.xyz) and connect an EVM wallet (MetaMask,
   WalletConnect, social login, etc.). This is the owner EOA that authorises the smart-
   contract wallet below.
2. **Register the Derive Chain smart-contract wallet.** First sign-in deploys a per-user
   smart-contract wallet on the Derive testnet chain. The address shown under
   "Developers" -> "Derive Wallet" is the `wallet_address` (and the `X-LYRAWALLET` header)
   the client uses. It is distinct from the EOA you just connected.
3. **Create a subaccount.** Open a subaccount under the wallet (Standard Margin is the
   simplest mode for test trading). The integer id is the `subaccount_id` the client signs
   each `private/order` request against.
4. **Generate a session key.** Under "Developers" -> "Session Keys", create a session key
   scoped to the subaccount and copy the raw secp256k1 private key. This is the
   `session_key` value; it never leaves the client and is redacted from `Debug` output.
   Session keys can be rotated or revoked from the same panel.
5. **Fund the subaccount via the faucet.** The testnet dashboard exposes a USDC faucet
   that drips test collateral. Deposit into the subaccount so the on-chain balance shows
   non-zero collateral; the API will reject orders until the subaccount has enough margin
   for the requested size.
6. **Set the environment variables.** Export the three values the client reads in testnet
   mode (or pass them on `DeriveExecClientConfig`, where the config field wins):

   ```bash
   export DERIVE_TESTNET_WALLET_ADDRESS="0x..."  # Derive Chain smart-contract wallet
   export DERIVE_TESTNET_SESSION_PRIVATE_KEY="0x..."  # secp256k1 session-key private key
   export DERIVE_TESTNET_SUBACCOUNT_ID="12345"  # integer subaccount id
   ```

### Minimum funding

There is no fixed venue minimum. The matching engine accepts any order that satisfies the
subaccount's initial-margin requirement for the resulting position. Treat these as
practical floors for the smallest viable test:

- **Smoke test (submit and cancel, no fills):** any positive USDC balance covers the
  signed-order plumbing.
- **Round-trip an `ETH-PERP` fill:** budget for the worst-case slippage-adjusted notional
  plus the initial-margin cushion. For one contract at $3500 and the venue's ~10% IM, that
  is roughly $350 collateral plus $400 cushion. Around $1000 USDC is a comfortable working
  balance for a first-fill test.
- **Options:** options carry higher IM than perps. Pull `public/get_instrument` for the
  option, multiply the contract size by mark price, then add the option-specific IM
  (visible on the instrument response) before sizing the deposit.

Use the `private/get_subaccount` endpoint after funding to confirm
`initial_margin`/`maintenance_margin` headroom against the order you plan to submit; the
adapter's `query_account` command emits this snapshot as an `AccountState` event so the
strategy layer can gate trading on it.

## Mainnet onboarding

Mainnet onboarding mirrors testnet against the production dashboard. Use real funds.

1. **Sign in to the mainnet dashboard.** Open [derive.xyz](https://derive.xyz) and connect
   the EVM owner wallet (MetaMask, WalletConnect, social login, etc.). First sign-in
   deploys your Derive Chain smart-contract wallet.
2. **Copy the wallet address.** Under "Developers" -> "Derive Wallet", copy the
   smart-contract wallet address. This is the `wallet_address` the client signs against; it
   is **distinct** from the EOA you signed in with. Verify on the Derive Chain explorer
   that the address has contract code (EOAs do not).
3. **Create or pick a subaccount.** Open a subaccount under the wallet (Standard Margin is
   the simplest mode; switch to Portfolio Margin only once you understand the cross-margin
   semantics). The integer id is the `subaccount_id`.
4. **Generate a mainnet session key.** Under "Developers" -> "Session Keys", create a
   session key scoped to the subaccount and copy the raw secp256k1 private key. Session
   keys can be rotated or revoked from the same panel; prefer short-lived keys for
   exploratory tester runs.
5. **Fund the subaccount.** Deposit USDC (or supported collateral) into the subaccount via
   the dashboard's deposit flow. Confirm via `private/get_subaccount` (or the adapter's
   `query_account`) that `collaterals_value` and `initial_margin` headroom cover the
   intended order before submitting.
6. **Set the environment variables.** Export the three mainnet values (or pass them on
   `DeriveExecClientConfig`, where the config field wins):

   ```bash
   export DERIVE_WALLET_ADDRESS="0x..."  # Derive Chain smart-contract wallet
   export DERIVE_SESSION_PRIVATE_KEY="0x..."  # secp256k1 session-key private key
   export DERIVE_SUBACCOUNT_ID="12345"  # integer subaccount id
   ```

   The `node_exec_tester` example is pinned to `DeriveEnvironment::Testnet`; flip
   the literal to `DeriveEnvironment::Mainnet` for real-funds runs. The
   `node_data_tester` and `node_delta_neutral` examples default to testnet and read
   `DERIVE_ENVIRONMENT=mainnet` to flip. Production deployments select the network via
   `DeriveDataClientConfig::environment` / `DeriveExecClientConfig::environment`.

## Capabilities

### Market data

| Capability                     | Supported | Notes                                                                   |
|--------------------------------|-----------|-------------------------------------------------------------------------|
| Request instrument (REST)      | ✓         | `public/get_instrument`; loads one instrument into the local cache.     |
| Request all instruments (REST) | ✓         | `public/get_instruments`; fetches each currency in `currencies`.        |
| Instrument subscription        | -         | *Not supported.* Use the configured REST refresh interval.              |
| Order book deltas (L2_MBP)     | ✓         | Channel: `orderbook.{instrument}.{group}.{depth}`.                      |
| Order book depth10 (L2_MBP)    | ✓         | Same order book channel with `depth=10`.                                |
| Order book at interval         | -         | *Not supported.* Maintain interval books from deltas locally.           |
| Order book snapshot (REST)     | -         | *Not supported.* Not exposed by the adapter.                            |
| Historical book deltas (REST)  | -         | *Not supported.* Not exposed by the adapter.                            |
| Quotes (`ticker_slim`)         | ✓         | Channel: `ticker_slim.{instrument}.{interval}`.                         |
| Quote snapshot (REST)          | ✓         | One‑shot `public/get_tickers`; emits a single `QuoteTick`.              |
| Historical quotes (REST)       | -         | *Not supported.* The venue exposes ticker snapshots only.               |
| Trades                         | ✓         | Channel: `trades.{instrument_type}.{currency}`.                         |
| Historical trades (REST)       | ✓         | `public/get_trade_history`; honors `start`, `end`, and `limit`.         |
| Bars / OHLC (REST)             | ✓         | `public/get_tradingview_chart_data`; minute, hour, day, and week bars.  |
| Bars / OHLC (WS)               | -         | *Not supported.* The venue has no candle subscription channel.          |
| Mark price stream              | ✓         | Derived from `ticker_slim`; shares the quote subscription.              |
| Index price stream             | ✓         | Derived from `ticker_slim`; shares the quote subscription.              |
| Funding rate stream            | ✓         | Derived from `perp_details.funding_rate` on perp tickers.               |
| Funding rate history (REST)    | ✓         | `public/get_funding_rate_history` for perpetuals.                       |
| Instrument status              | -         | *Not supported.* Ticker payloads include `is_active`.                   |
| Instrument close               | -         | *Not supported.* Option settlement is REST-only.                        |
| Option greeks                  | ✓         | Derived from `option_pricing` on option tickers.                        |
| Option chain                   | ✓         | Aggregated from quotes and greeks; `public/get_tickers` bootstraps ATM. |

`request_instrument` calls `public/get_instrument` for the requested `InstrumentId` and
caches the returned definition before emitting the response. The cached instrument carries
the precision and increment fields used by later quote, trade, book, and bar parsing.

Derive exposes book deltas and depth10 snapshots through the same
`orderbook.{instrument}.{group}.{depth}` channel family. `subscribe_book_deltas` publishes
snapshot deltas as `OrderBookDeltas`, while `subscribe_book_depth10` fixes `depth=10` and
publishes `OrderBookDepth10` snapshots.

### Execution

Order placement, cancellation, modification, query, and report generation use Derive's
EIP-712 self-custodial signing flow. Order-entry writes (`private/order`, `private/cancel`,
`private/cancel_all`, `private/replace`) go over the WebSocket Trading API on the same
authenticated session that streams account, order, trade, and balance state through the
private channels (`{subaccount_id}.orders`, `{subaccount_id}.trades`,
`{subaccount_id}.balances`). The signed EIP-712 body is identical regardless of transport.

:::note
The HTTP order-entry endpoints remain available on `DeriveHttpClient` for tooling and tests,
but the live execution client routes all writes over the WebSocket Trading API. Report
generation, account refresh, and instrument lookups still use REST.
:::

Perpetuals, options, and ERC-20 spot pairs all use the Derive Trade module. Spot has no
separate signing path, and reconciliation treats spot instruments like other instrument
classes except for the reduce-only guard described below.

The adapter supports ordinary `private/order` requests: `LIMIT` and `MARKET` orders with
`GTC`, `IOC`, or `FOK` time-in-force values. It also supports Derive trigger orders for the
Nautilus-native stop and if-touched order types listed below. Unsupported Nautilus order
types are rejected before signing, so they cannot fill at the venue.

Market orders require a cached quote before submission. After the async submit task resolves the
instrument, it refreshes the current ticker snapshot and derives the signed slippage-bound
`limit_price` from that refreshed quote.

#### Conditional orders

Derive trigger orders use the WebSocket-only `private/trigger_order` endpoint, not the normal
`private/order` endpoint. The venue stores them with `order_status=untriggered` until its
trigger worker submits the signed child order. Reconciliation therefore reads both
`private/get_open_orders` and `private/get_trigger_orders`.

Derive mainnet requires trigger-order signatures to expire 30 to 90 days from venue time. The
adapter signs trigger orders with a fixed 31-day expiry; `signature_expiry_secs` still controls
ordinary `private/order` and `private/replace` writes, and must be greater than the 300s venue
minimum.

| Nautilus order type | Supported | Derive `order_type` | Derive `trigger_type` | Notes                         |
|---------------------|-----------|---------------------|-----------------------|-------------------------------|
| `StopMarket`        | ✓         | `market`            | `stoploss`            | Uses trigger price as bound.  |
| `StopLimit`         | ✓         | `limit`             | `stoploss`            | Sends limit and trigger price. |
| `MarketIfTouched`   | ✓         | `market`            | `takeprofit`          | Uses trigger price as bound.  |
| `LimitIfTouched`    | ✓         | `limit`             | `takeprofit`          | Sends limit and trigger price. |
| `MarketToLimit`     | -         | -                   | -                     | *Not supported by Derive*.    |
| Trailing stops      | -         | -                   | -                     | *Not supported by Derive*.    |
| TWAP / algo / RFQ   | -         | -                   | -                     | *Not exposed by this adapter*. |

The adapter maps Nautilus `TriggerType::Default` and `TriggerType::MarkPrice` to Derive
`trigger_price_type=mark`. Derive's current error-code reference states that index and
last-trade trigger price types are not supported yet, so `IndexPrice`, `LastPrice`, `BidAsk`,
and other trigger price types are rejected locally before signing.

Derive error `11054` states that trigger orders cannot replace or be replaced. The adapter
therefore rejects Nautilus modify requests for trigger orders with an `OrderModifyRejected`
event; cancel and resubmit for trigger updates.

#### Execution instructions

| Instruction   | Supported | Derive value  | Notes                                                         |
|---------------|-----------|---------------|---------------------------------------------------------------|
| `post_only`   | ✓         | `post_only`   | Requires `GTC`; rejects if the order would take liquidity.    |
| `reduce_only` | ✓         | `reduce_only` | Supported for perps and options. Spot is rejected locally.    |

#### Time in force

Derive documents `gtc`, `post_only`, `fok`, and `ioc` as its `time_in_force` values. The
adapter rejects Nautilus values with no Derive equivalent before signing. Derive exposes
post-only as a `time_in_force` value, so `post_only` cannot combine with `IOC` or `FOK`.

| Time in force  | Supported | Derive value | Notes                      |
|----------------|-----------|--------------|----------------------------|
| `GTC`          | ✓         | `gtc`        | Good Till Canceled.        |
| `IOC`          | ✓         | `ioc`        | Immediate or Cancel.       |
| `FOK`          | ✓         | `fok`        | Fill or Kill.              |
| `GTD`          | -         | -            | *Not supported by Derive*. |
| `DAY`          | -         | -            | *Not supported by Derive*. |
| `AT_THE_OPEN`  | -         | -            | *Not supported by Derive*. |
| `AT_THE_CLOSE` | -         | -            | *Not supported by Derive*. |

#### Spot reduce-only orders

Derive spot has no position concept, so a reduce-only spot order can never reduce anything.
The venue always rejects it with error `11025`; the adapter avoids that round-trip when it
knows the instrument is spot. Cached spot instruments are denied with `OrderDenied`; lazily
resolved spot instruments are rejected with `OrderRejected` during submit.

Reduce-only orders for perpetuals and options still reach the venue, where the outcome
depends on the subaccount's position state. The `derive-flatten` bin closes derivative
positions only and never spot, since flattening a spot balance would dump the base asset
into a different quote.

#### Order rejection semantics

State-changing writes (`submit_order`, `modify_order`, `cancel_order`) are sent once over the
WebSocket Trading API and are not replayed. The adapter keys terminal vs ambiguous handling
off the WebSocket request outcome. It emits a terminal rejection event (`OrderRejected`,
`OrderModifyRejected`, `OrderCancelRejected`) for definitive venue failures:

- Signed-action rejections such as invalid params, insufficient margin, or unknown orders.
- Venue business codes such as `11009 Zero liquidity`.
- Post-only crossing rejections (`11008 Post only order cannot cross the market`), reported
  as `OrderRejected` with `due_post_only=true`.
- Rate-limit responses (`-32000 Rate limit exceeded`), where the gateway rejects the request
  before the matching engine sees it.

For post-only orders that reach the venue, Derive rejects a crossing order with JSON-RPC
`11008` and message `Post only order cannot cross the market`. The adapter marks that
terminal rejection with `due_post_only=true`; if a WebSocket/order-report rejection carries
the same reason, the tracked order path applies the same classification. Local rejections
for unsupported post-only IOC/FOK combinations are not marked `due_post_only` because they
do not represent a venue crossing rejection.

For ambiguous write outcomes, the adapter emits no terminal event and lets WebSocket
reconciliation or later status reports settle the state. The ambiguous set is deliberately
narrow:

- `-32603`, a generic JSON-RPC internal error.
- A response that cannot be decoded (the action may have been processed).
- Request timeouts, dropped responses on reconnect, and transport errors.

This distinction protects both sides of the order lifecycle. A false terminal rejection can
make the engine treat a live order as rejected; a false ambiguous outcome can leave an
unplaced order hanging in `Submitted` forever because no WebSocket frame will arrive.

## Subscription parameters

`subscribe_book_deltas` and `subscribe_book_depth10` accept these `subscribe_params` keys:

| Key      | Type   | Default | Allowed              |
|----------|--------|---------|----------------------|
| `group`  | string | `"1"`   | `"1"`, `"10"`, `"100"` |
| `depth`  | string | `"10"`  | `"1"`, `"10"`, `"20"`, `"100"` |

`subscribe_quotes` accepts:

| Key        | Type   | Default  | Allowed           |
|------------|--------|----------|-------------------|
| `interval` | string | `"1000"` | `"100"`, `"1000"` |

Unknown values are rejected at subscribe time.

### Shared ticker subscription

Quotes, mark prices, index prices, funding rates, and option greeks are all derived from the
same `ticker_slim.{instrument}.{interval}` WebSocket subscription. The adapter reference-counts
the underlying WS subscribe call: the first feed subscribed for an instrument opens the channel
and the last unsubscribe closes it. As a consequence, the `interval` from the first subscribe
wins; subsequent feeds subscribing with a different interval share the existing channel.

Mark prices, index prices, funding rates, and option greeks all read fields that the venue
includes in the full ticker payload (`mark_price`, `index_price`, `perp_details.funding_rate`,
`option_pricing`). Observed Derive pushes on `ticker_slim` carry these fields, so the derived
feeds work. If the venue ever pushes the compact `SlimEnvelope` shape on this channel, those
derived feeds will silently produce no data for that frame; the quote feed still works because
bid/ask are present in both shapes.

Funding rates are only meaningful for perpetuals, and option greeks only for options.
Subscribing the wrong feed for an instrument's class (e.g. funding rates for an option) is
accepted and the WebSocket subscription opens, but the parser returns no events for that feed
because the venue payload lacks the relevant fields (`perp_details` for non-perps,
`option_pricing` for non-options). Verify the instrument class before subscribing to derivative-
specific feeds.

## Configuration

### Data client configuration options

Class/struct: `DeriveDataClientConfig`.

| Option                             | Default   | Description |
|------------------------------------|-----------|-------------|
| `base_url_rest`                    | `None`    | Override for the REST base URL. |
| `base_url_ws`                      | `None`    | Override for the WebSocket base URL. |
| `proxy_url`                        | `None`    | Optional proxy URL for HTTP and WebSocket transports. |
| `environment`                      | `Mainnet` | Network selector (`MAINNET` or `TESTNET` in Python). |
| `http_timeout_secs`                | `10`      | REST request timeout in seconds. |
| `ws_timeout_secs`                  | `30`      | WebSocket connect and idle timeout in seconds. |
| `update_instruments_interval_mins` | `60`      | Interval in minutes between instrument refreshes. |
| `currencies`                       | `[]`      | Currencies to bulk‑load on connect. Empty means lazy‑load on demand. |
| `include_expired`                  | `false`   | Include expired option rows from `public/get_instruments`. |
| `auto_load_missing_instruments`    | `true`    | Lazy‑load unknown instruments before subscribe or request commands. |
| `transport_backend`                | `Sockudo` | WebSocket transport when `transport-sockudo` is enabled. |

### Execution client configuration options

Class/struct: `DeriveExecClientConfig`.

| Option                      | Default   | Description |
|-----------------------------|-----------|-------------|
| `wallet_address`            | `None`    | Derive Chain smart‑contract wallet address. Falls back to env vars below. |
| `session_key`               | `None`    | secp256k1 session‑key private key. Falls back to env vars below. |
| `subaccount_id`             | `None`    | Derive subaccount id. Falls back to env vars below. |
| `base_url_rest`             | `None`    | Override for the REST base URL. |
| `base_url_ws`               | `None`    | Override for the WebSocket base URL. |
| `proxy_url`                 | `None`    | Optional proxy URL for HTTP and WebSocket transports. |
| `environment`               | `Mainnet` | Network selector (`MAINNET` or `TESTNET` in Python). |
| `http_timeout_secs`         | `10`      | REST request timeout in seconds. |
| `max_retries`               | `3`       | Retry attempts for recoverable reads and definitive non‑write paths. |
| `retry_delay_initial_ms`    | `100`     | Initial retry delay in milliseconds. |
| `retry_delay_max_ms`        | `5000`    | Maximum retry delay in milliseconds. |
| `max_fee_per_contract`      | `None`    | Per‑contract USDC fee cap signed into each order. |
| `domain_separator`          | `None`    | Optional EIP-712 domain separator override. |
| `action_typehash`           | `None`    | Optional EIP-712 action typehash override. |
| `trade_module_address`      | `None`    | Optional Trade module contract address override. |
| `signature_expiry_secs`     | `600`     | Order/replace TTL; must be >300s. Trigger orders use fixed 31-day TTL. |
| `market_order_slippage_bps` | `50`      | Slippage bound for market‑order limit prices. |
| `transport_backend`         | `Sockudo` | WebSocket transport when `transport-sockudo` is enabled. |

The default transport falls back to `Tungstenite` when the build disables the
`transport-sockudo` feature.

The `wallet_address`, `session_key`, and `subaccount_id` fall back to environment variables when
unset:

| Field            | Mainnet variable             | Testnet variable                     |
|------------------|------------------------------|--------------------------------------|
| `wallet_address` | `DERIVE_WALLET_ADDRESS`      | `DERIVE_TESTNET_WALLET_ADDRESS`      |
| `session_key`    | `DERIVE_SESSION_PRIVATE_KEY` | `DERIVE_TESTNET_SESSION_PRIVATE_KEY` |
| `subaccount_id`  | `DERIVE_SUBACCOUNT_ID`       | `DERIVE_TESTNET_SUBACCOUNT_ID`       |

The session key is the secp256k1 private key registered on the wallet for API signing. The
`session_key` field is redacted in `Debug` output and Python `repr`.

### Python v2 live node

Rust-backed Python v2 nodes use `LiveNode.builder(...)` and pass concrete factory
instances. The execution factory needs `DeriveExecFactoryConfig`, which wraps the trader
and account identifiers with the underlying `DeriveExecClientConfig`.

```python
from decimal import Decimal

from nautilus_trader.adapters.derive import DeriveDataClientConfig
from nautilus_trader.adapters.derive import DeriveDataClientFactory
from nautilus_trader.adapters.derive import DeriveEnvironment
from nautilus_trader.adapters.derive import DeriveExecClientConfig
from nautilus_trader.adapters.derive import DeriveExecFactoryConfig
from nautilus_trader.adapters.derive import DeriveExecutionClientFactory
from nautilus_trader.common import Environment
from nautilus_trader.live import LiveNode
from nautilus_trader.model import AccountId
from nautilus_trader.model import TraderId

trader_id = TraderId("TESTER-001")

data_config = DeriveDataClientConfig(
    environment=DeriveEnvironment.TESTNET,
    currencies=["ETH", "BTC"],
)

exec_config = DeriveExecClientConfig(
    environment=DeriveEnvironment.TESTNET,
    max_fee_per_contract=Decimal("1000"),
)

exec_factory_config = DeriveExecFactoryConfig(
    trader_id,
    AccountId("DERIVE-001"),
    exec_config,
)

node = (
    LiveNode.builder("DERIVE-001", trader_id, Environment.LIVE)
    .add_data_client(None, DeriveDataClientFactory(), data_config)
    .add_exec_client(None, DeriveExecutionClientFactory(), exec_factory_config)
    .build()
)
```

Do not pass `DeriveExecClientConfig` directly to `add_exec_client`; the Derive execution
factory requires the wrapped `DeriveExecFactoryConfig` so it can create the
`ExecutionClientCore` with the correct trader and account identifiers.

### Rust data client

```rust
use nautilus_derive::{
    common::enums::DeriveEnvironment,
    config::DeriveDataClientConfig,
};

let config = DeriveDataClientConfig {
    environment: DeriveEnvironment::Testnet,
    currencies: vec!["ETH".to_string(), "BTC".to_string()],
    ..Default::default()
};
```

Notable fields:

- `currencies`: which currencies to bulk-load on connect. Empty means lazy-load per subscribe.
- `include_expired`: include expired option rows from `public/get_instruments`.
- `auto_load_missing_instruments`: lazy-load on subscribe when an instrument is unknown.
- `update_instruments_interval_mins`: REST refresh interval (default 60 minutes).
- `http_timeout_secs`, `ws_timeout_secs`: transport timeouts.

### Rust execution client

```rust
use nautilus_derive::{
    common::enums::DeriveEnvironment,
    config::DeriveExecClientConfig,
};

let config = DeriveExecClientConfig {
    wallet_address: Some("0x...".to_string()),
    session_key: Some("0x...".to_string()),
    subaccount_id: Some(1),
    environment: DeriveEnvironment::Testnet,
    ..Default::default()
};
```

## Known limitations

- `request_instruments` requires at least one configured currency in
  `DeriveDataClientConfig::currencies`; the venue's `public/get_instruments` endpoint is
  scoped per-currency and the adapter does not enumerate the currency universe.
- The `data_client.rs` integration test asserts the set, not the order, of recorded REST
  calls because `fetch_instrument_definitions` issues the `perp` and `option` requests in
  parallel via `tokio::try_join!`.
- Venue does not push instrument status, instrument close, or candle subscriptions; the
  ticker payload carries margin parameters and `is_active`, and bars are REST-only.
- The book snapshot REST endpoint and historical book deltas / historical quote endpoints
  are not exposed by the venue. See the capabilities table above.
- Derive's official REST docs mark `public/get_ticker` as deprecated in favor of
  `public/get_tickers` as of December 1, 2025. The adapter uses `public/get_tickers`
  for quote snapshots and option-chain forward-price bootstrap.
