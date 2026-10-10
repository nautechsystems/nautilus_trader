# Lighter

[Lighter](https://lighter.xyz) is a decentralized central-limit-order-book exchange for spot and
perpetual futures. The venue settles through an Ethereum zero-knowledge rollup, while matching and
sequencing run off-chain. The adapter also supports the Robinhood Chain deployment of the Lighter
protocol.

The `nautilus-lighter` crate provides Rust data and execution clients, typed REST and WebSocket
models, and an in-tree L2 transaction signer using Schnorr signatures over ECgFp5. Use this page to
configure either deployment and check its capabilities and operational limits.

L2 signing benchmarks, including a comparison with the official Go SDK, are recorded in
[`crates/adapters/lighter/benches/BENCHMARKS.md`](../../crates/adapters/lighter/benches/BENCHMARKS.md).
Absolute numbers vary by machine, so only same-machine deltas are meaningful.

## Overview

The main components are:

- `LighterRawHttpClient`: low-level REST client for the public and account endpoints.
- `LighterHttpClient`: domain client that parses instruments, trades, books, orders, and account
  state into Nautilus model types.
- `LighterWebSocketClient`: reconnecting WebSocket client for public market and private account streams.
- `LighterDataClient`: Nautilus data client for instruments, trades, quotes, and L2 market-by-price (MBP) books.
- `LighterExecutionClient`: Nautilus execution client for account streams, order submission,
  modification, cancellation, and reconciliation reports.
- `LighterDataClientFactory` and `LighterExecutionClientFactory`: live-node factory wiring.

The Python extension exposes configuration, deployment and environment selection, factories, and
integrator revocation. Live nodes consume the data and execution clients through Rust traits.

## Examples

Edit each tester's module-level constants in its source before running it, including
`LIGHTER_DEPLOYMENT` and `LIGHTER_ENVIRONMENT`. Both Rust and Python testers connect and start
immediately; these selectors do not read environment variables.

:::warning
Both execution testers enable order submission by default: Rust sets `DRY_RUN = false` and selects
Lighter Mainnet; Python sets `DRY_RUN = False` and selects Lighter Testnet. On either mainnet deployment,
a funded account can place real orders. Review the instrument, quantity, deployment, and environment
before running a tester, or enable `DRY_RUN` to connect without submitting orders.
:::

Python examples are in
[`examples/live/lighter/`](https://github.com/nautechsystems/nautilus_trader/tree/develop/examples/live/lighter/).

From the repository root:

```bash
uv run --project python --no-sync python examples/live/lighter/data_tester.py
uv run --project python --no-sync python examples/live/lighter/exec_tester.py
```

Rust examples are in `crates/adapters/lighter/examples/`:

```bash
cargo run --example lighter-data-tester --package nautilus-lighter --features examples
cargo run --example lighter-exec-tester --package nautilus-lighter --features examples
```

### Emergency account cleanup

`cargo run --bin lighter-flatten -p nautilus-lighter` submits an immediate account-wide cancellation,
reads one position snapshot, and submits reduce-only immediate-or-cancel (IOC) closes for the
positions in that snapshot.

:::warning
Stop other writers for the account before running this command. Cleanup is account-wide, not
strategy-scoped, so review the active account and positions first.
:::

The command does not confirm or retry requests. Success means it submitted the cancellation and
discovered closes without a known error, not that the account is flat. Check account state afterward
and rerun if anything remains. Each run submits at most 15 closes: cancellation uses one slot in
the 16-transaction nonce window. An incomplete snapshot, request failure, or submission failure
returns an error.

Set `LIGHTER_DEPLOYMENT` to `lighter` or `robinhood` and `LIGHTER_ENVIRONMENT` to `mainnet` or
`testnet`; omitted selectors default to Lighter Mainnet and select the matching credential
namespace.

## Product support

| Product type      | Data feed | Trading | Notes                                                     |
| ----------------- | --------- | ------- | --------------------------------------------------------- |
| Spot              | ✓         | ✓       | Spot markets; new listings use 64-bit ids from 4095.      |
| Perpetual futures | ✓         | ✓       | Linear perpetuals; new listings use 64-bit ids from 4095. |
| Dated futures     | -         | -       | *Not supported*.                                          |
| Options           | -         | -       | *Not supported*.                                          |

## Limitations

The adapter has these limits:

- Grouped order lists, OCO/OTO groups, brackets, TWAP, trailing stops, and iceberg display size are
  not implemented. Batch submit does not use `CreateGroupedOrders`.
- Order lists submit up to 15 independent transactions sequentially over WebSocket. Explicit batch
  cancellation also accepts at most 15 transactions per command.
- The execution client implements `CancelAllOrders` from cached open orders filtered by the requested
  instrument and optional `order_side`, across strategies. Each cancellation retains the order's
  owning strategy. The native cancel-all transaction cannot enforce the side filter, and the local
  signing schema has no market restriction. Explicit per-order cancellations preserve these filters
  and are sent in WebSocket batches of up to 15 transactions.
- Spot trading supports market and limit orders. Conditional stop-loss and take-profit orders are
  limited to perpetual markets.
- Account state and position reports come from private WebSocket streams. `query_account` and
  position status generation replay the latest cached stream state.
- Unscoped order reconciliation is bounded to configured or observed active markets to avoid a full
  venue-wide fan-out under the standard REST quota.

## Symbology

Lighter identifies markets by numeric `market_index` values in the venue's 64-bit allocation.
Existing markets keep their range-partitioned IDs. Following
[Lighter Mainnet's September 2026 upgrade](https://t.me/lighter_api_updates/174) and
[Robinhood's later upgrade](https://t.me/lighter_api_updates/184), new spot and perpetual markets
take the next free index at or above `4095`. Product type always comes from the venue's
`market_type` field, never from the index. The adapter bootstraps the mapping from
`GET /api/v1/orderBookDetails`, then converts the raw venue symbol into a Nautilus `InstrumentId`.

| Deployment product  | Nautilus symbol format                  | Example                            | Notes                    |
| ------------------- | --------------------------------------- | ---------------------------------- | ------------------------ |
| Lighter perpetual   | `{BASE}-PERP.LIGHTER`                   | `BTC-PERP.LIGHTER`                 | Raw venue symbol `BTC`.  |
| Lighter spot        | `{BASE}/{QUOTE}-SPOT.LIGHTER`           | `ETH/USDC-SPOT.LIGHTER`            | Raw symbol `ETH/USDC`.   |
| Robinhood perpetual | `{BASE}-PERP.LIGHTER_ROBINHOOD`         | `SNDK-PERP.LIGHTER_ROBINHOOD`      | Raw venue symbol `SNDK`. |
| Robinhood spot      | `{BASE}/{QUOTE}-SPOT.LIGHTER_ROBINHOOD` | `SNDK/USDG-SPOT.LIGHTER_ROBINHOOD` | Raw symbol `SNDK/USDG`.  |

The suffix separates spot and perpetual listings. Outbound requests strip it and use the cached
`market_index`; spot symbols retain the venue pair.

## Deployments and environments

| Deployment | Environment | REST URL                              | WebSocket URL                              | L2 signing chain ID | Settlement | Default venue       |
| ---------- | ----------- | ------------------------------------- | ------------------------------------------ | ------------------- | ---------- | ------------------- |
| Lighter    | Mainnet     | `https://mainnet.zklighter.elliot.ai` | `wss://mainnet.zklighter.elliot.ai/stream` | 304                 | USDC       | `LIGHTER`           |
| Lighter    | Testnet     | `https://testnet.zklighter.elliot.ai` | `wss://testnet.zklighter.elliot.ai/stream` | 300                 | USDC       | `LIGHTER`           |
| Robinhood  | Mainnet     | `https://api.rh.lighter.xyz`          | `wss://api.rh.lighter.xyz/stream`          | 466324              | USDG       | `LIGHTER_ROBINHOOD` |
| Robinhood  | Testnet     | `https://api.rh-testnet.lighter.xyz`  | `wss://api.rh-testnet.lighter.xyz/stream`  | 300                 | USDG       | `LIGHTER_ROBINHOOD` |

These chain IDs are Lighter L2 signing-domain values, not EVM network chain IDs.

Select the protocol deployment with `LighterDeployment::Lighter` or `LighterDeployment::Robinhood`,
and its environment with `LighterEnvironment::Mainnet` or `LighterEnvironment::Testnet`.
Together, they control default URLs, chain ID, settlement currency, venue, and attribution policy.
Both testnets use chain ID 300, so the adapter never infers deployment behavior from the numeric ID.

URL overrides for private gateways and local test fixtures replace only the transport endpoint.
The selected deployment and environment still control signing, settlement currency, and attribution.

### Custom venue identity

Set `venue` on both data and execution configs when separate Lighter-protocol endpoints must have
distinct Nautilus identities. This scopes instruments, cache entries, message topics, socket state,
and execution routing without changing the selected deployment's protocol behavior. `ClientId`
remains the name supplied when registering each client.

The shared factory name remains `LIGHTER` for compatibility. For Robinhood routing by `ClientId`,
register the client as `LIGHTER_ROBINHOOD`; both language examples derive it from `LIGHTER_DEPLOYMENT`.
Custom client names are also supported.

The `account_id` issuer must match the resolved venue because Nautilus routes account commands by
issuer. Venue `LIGHTER_RH_ALT`, for example, requires an account ID such as
`LIGHTER_RH_ALT-001`. A custom venue does not enable a custom chain ID or custom attribution.

## Account and API key setup

Public market data does not require an account. Private account streams and execution require an
account index, an API key index, and the API private key from the same deployment and environment.
Each row below has a separate account and API-key namespace:

| Deployment | Environment | Account and API key page                                        | Account issuer      | Credential prefix             |
| ---------- | ----------- | --------------------------------------------------------------- | ------------------- | ----------------------------- |
| Lighter    | Mainnet     | [Lighter Mainnet](https://app.lighter.xyz/apikeys)              | `LIGHTER`           | `LIGHTER_*`                   |
| Lighter    | Testnet     | [Lighter Testnet](https://testnet.app.lighter.xyz/apikeys)      | `LIGHTER`           | `LIGHTER_TESTNET_*`           |
| Robinhood  | Mainnet     | [Robinhood Mainnet](https://robinhoodchain.lighter.xyz/apikeys) | `LIGHTER_ROBINHOOD` | `LIGHTER_ROBINHOOD_*`         |
| Robinhood  | Testnet     | [Robinhood Testnet](https://rhctestnet.lighter.xyz/apikeys)     | `LIGHTER_ROBINHOOD` | `LIGHTER_ROBINHOOD_TESTNET_*` |

Do not mix an account index or API key from one row with another. This also applies to the two
testnets even though both use L2 signing chain ID 300.

1. Open the target deployment's account page and sign in. Create or select the trading account,
   including the intended sub-account, before generating its API key.
1. Follow Lighter's
   [account-index lookup](https://apidocs.lighter.xyz/docs/get-started#find-your-account-index)
   against the target deployment's REST URL. This example selects Robinhood Mainnet; replace the
   URL with the exact value from the [deployment table](#deployments-and-environments) for another
   row:

   ```bash
   LIGHTER_SETUP_API_URL="https://api.rh.lighter.xyz"
   LIGHTER_SETUP_L1_ADDRESS="0xYOUR_ETHEREUM_ADDRESS"

   curl -sS --get \
     "${LIGHTER_SETUP_API_URL}/api/v1/accountsByL1Address" \
     --data-urlencode "l1_address=${LIGHTER_SETUP_L1_ADDRESS}"
   ```

   Read the `index` from the required entry in `sub_accounts`. A wallet can own a main account and
   several sub-accounts, each with a separate account index and API keys.
1. On the selected account's API key page, choose **Generate API Key**. Use an unused index in
   `[4, 254]`. Lighter's [API key documentation](https://apidocs.lighter.xyz/docs/api-keys) reserves
   `[0, 3]` for its interfaces, and `255` is an API query sentinel. Robinhood also reserves `157`;
   see [Robinhood API keys](https://apidocs.lighter.xyz/docs/lighter-rh#api-keys).
1. Save the generated private key before closing the dialog. Lighter does not display it again.
1. Configure `account_index`, `api_key_index`, and `private_key` directly, or use the environment
   variables listed in [API credentials](#api-credentials). The Nautilus `account_id` is separate
   from the venue account index: use an issuer from the table above, such as `LIGHTER-001` or
   `LIGHTER_ROBINHOOD-001`.
1. Confirm that the target deployment recognizes the selected account and key indexes:

   ```bash
   LIGHTER_SETUP_ACCOUNT_INDEX="123456"
   LIGHTER_SETUP_API_KEY_INDEX="4"

   curl -sS --get \
     "${LIGHTER_SETUP_API_URL}/api/v1/apikeys" \
     --data-urlencode "account_index=${LIGHTER_SETUP_ACCOUNT_INDEX}" \
     --data-urlencode "api_key_index=${LIGHTER_SETUP_API_KEY_INDEX}"
   ```

   A successful response has `"code": 200` and lists the selected key. This public lookup confirms
   the indexes, but it does not expose or validate the private key.

:::warning
Lighter API keys authorize trading, private account access, and some withdrawal operations. Store
the private key in a secret manager or protected environment configuration. Do not commit it to a
repository or share it in logs.
:::

## Integrator attribution

NautilusTrader participates in [Lighter's partner program](https://apidocs.lighter.xyz/docs/partner-integration).
On Lighter Mainnet, create and modify order transactions from the execution client carry the
NautilusTrader integrator account index in `L2TxAttributes` across all account tiers, including
Standard. This includes each order submitted through an order list. Maker and taker integrator fees
are zero.

Zero-fee attribution requires an `ApproveIntegrator` transaction, which needs only an L2 signature.
During startup, the execution client submits this approval with all maximum fee limits set to zero
when the API key is not maker-only.

Lighter Testnet and both Robinhood environments leave `L2TxAttributes` empty and omit
`ApproveIntegrator` during startup.

Robinhood Mainnet uses the account-level `NAUTILUS` referral code. Selecting it opts the account
into this attribution: at startup, the client authenticates with the configured L2 API key and
applies the code to the account's public L1 address. Failures log a warning and do not block trading.
Robinhood Testnet performs no referral attribution.

Custom venue names do not change either policy: attribution is evaluated from the typed deployment
and environment.

On Lighter Mainnet, maker-only API keys cannot submit `ApproveIntegrator`. The execution client
detects these keys and skips automatic approval. Approval is account-scoped, so a non-maker-only
key on the same account must approve the integrator before a maker-only key can trade through the
adapter.

### Revoking the approval

To revoke an existing approval when leaving the adapter on Lighter Mainnet, send `ApproveIntegrator`
with `approval_expiry = 0` and zero maximum fees.
The next Lighter Mainnet execution-client startup with a non-maker-only key records a new zero-fee
approval, regardless of account tier.

```bash
export LIGHTER_API_KEY_INDEX=5
export LIGHTER_API_SECRET=REPLACE_ME
export LIGHTER_ACCOUNT_INDEX=123456
cargo run -p nautilus-lighter --bin lighter-integrator-revoke           # Lighter Mainnet
```

Script source:
[`crates/adapters/lighter/bin/integrator_revoke.rs`](https://github.com/nautechsystems/nautilus_trader/blob/develop/crates/adapters/lighter/bin/integrator_revoke.rs).

```python
# Python (PyO3 binding) - reads the same env vars as the Rust bin
from nautilus_trader.adapters.lighter import revoke_lighter_integrator

await revoke_lighter_integrator()  # Lighter Mainnet (default)
```

The Rust command displays the action and waits for Enter before signing or sending. Abort with
`Ctrl+C` if the summary is wrong. The Python binding does not prompt; review the active environment
variables before calling it.

## Data subscriptions

| Data type            | Sub.         | Snapshot | Hist. | Nautilus type       | Notes                                                    |
| -------------------- | ------------ | -------- | ----- | ------------------- | -------------------------------------------------------- |
| Instrument metadata  | Cache replay | ✓        | -     | `InstrumentAny`     | Loaded from `orderBookDetails`.                          |
| Trade ticks          | ✓            | -        | ✓     | `TradeTick`         | WebSocket trades; public `recentTrades` REST history.    |
| Quote ticks          | ✓            | -        | -     | `QuoteTick`         | Best bid and ask ticker stream.                          |
| Order book deltas    | ✓            | ✓        | -     | `OrderBookDeltas`   | `L2_MBP` only.                                           |
| Order book depth     | ✓            | -        | -     | `OrderBookDepth`    | Live top-10 view from maintained book; no REST snapshot. |
| Order book snapshots | -            | ✓        | -     | `OrderBook`         | REST snapshot, max depth 250.                            |
| Mark prices          | ✓            | -        | -     | `MarkPriceUpdate`   | Perp market stats stream.                                |
| Index prices         | ✓            | -        | -     | `IndexPriceUpdate`  | Market and spot stats streams.                           |
| Funding rates        | ✓            | -        | ✓     | `FundingRateUpdate` | Current estimates and REST hourly history.               |
| Bars                 | ✓            | -        | ✓     | `Bar`               | WebSocket candle stream; REST history for backfill.      |
| Instrument status    | REST         | ✓        | -     | `InstrumentStatus`  | `active` / `inactive` snapshots.                         |

### Order book data

Book-delta and depth subscriptions accept only `BookType::L2_MBP`. Other book types return an
error before subscribing.

The WebSocket book initializes only from `subscribed/order_book`. Until that snapshot arrives, the
adapter drops `update/order_book` frames and emits no book data: incrementals omit unchanged levels.

Depth subscriptions use the same WebSocket `order_book` stream as deltas. The adapter emits a
refreshed top-10 view after each accepted snapshot or incremental update.

### Bars

Bar subscriptions use `candle/{market_id}/{resolution}`. Lighter batches open-candle updates every
~500 ms. The adapter emits a `Bar` only when the candle start timestamp advances, giving consumers
one event per closed period. Reconnect and unsubscribe clear the in-progress cache.

The stream supports `1m`, `5m`, `15m`, `30m`, `1h`, `4h`, `12h`, and `1d`. `1w` is REST-only via
`request_bars`; subscribing to a `1-WEEK` bar type returns an error.

REST bar history omits venue gap rows whose open, high, low, or close is missing, null, zero, or
negative. These rows cannot form valid Nautilus bars and do not stop later valid rows from loading.

### Instrument status and trades

Instrument status subscriptions replay cached `orderBookDetails` status or fetch a REST snapshot.
Lighter exposes no WebSocket status-change stream.

Trade subscriptions use the public WebSocket trade stream. Historical trade requests use the
public `/api/v1/recentTrades` endpoint without credentials. The adapter requests at most 100 trades
and filters them to the requested time range; it does not paginate this endpoint.

See [Funding rates](#funding-rates) for live and historical funding semantics.

### Unsupported data requests

`request_quotes` is not implemented: the adapter's REST endpoints provide no timestamped quote
snapshot or history that can map safely to `QuoteTick`. Subscribe to the WebSocket `ticker` stream
for live best bid and offer data.

`request_book_depth` is not implemented: the REST book endpoints provide no venue event timestamp
for `OrderBookDepth.ts_event`. Use `subscribe_book_depth` for live depth or `request_book_snapshot`
for a REST `OrderBook` snapshot.

## Order book recovery

### Sequence validation

The adapter checks each incremental update's `begin_nonce` against the previous book's `nonce`.
A mismatch suppresses book output and starts an unsubscribe/subscribe replacement.

The venue's `offset` is not a continuity counter: it can skip values and change across servers on
reconnect. See the [Lighter order book contract](https://apidocs.lighter.xyz/docs/websocket-reference#order-book).

### Snapshot requirements

Initial and replacement subscriptions wait up to `book_snapshot_timeout_secs` (default 10 seconds)
for a typed `subscribed/order_book` snapshot after the subscription write completes. Set it to `0`
to disable snapshot deadlines. A missing snapshot starts or retries recovery, including when a
control acknowledgement or `Already Subscribed` response arrives without a book.
Control acknowledgements release subscription slots but do not complete book recovery.

Book output resumes only after a matching snapshot replaces the cached levels. An empty snapshot
clears the book too.

### Retry limits and reconnects

Each recovery episode makes up to eight replacement attempts within 180 seconds, with
exponential backoff and jitter, then continues at an interval that doubles from one minute to
fifteen minutes until a snapshot is accepted. Replacement unsubscribe and subscribe writes target
the same connection.

Reconnect retires obsolete subscription generations and preserves running recovery and its remaining
budget. A recovery waiting between attempts after exhausting its budget retries immediately on the
new connection.

### Consumers and persistent failures

Deltas and depth share a recovery episode for each market:

- Removing one consumer preserves the other.
- Removing the final consumer cancels pending writes and snapshot waits.
- Shutdown cancels all owned work.

Recovery never ends in a failed state. Rejected replacements and rejected subscriptions replayed
after reconnect keep recovering at the growing interval; a late snapshot still restores the book.

A venue subscription failure other than rate limiting fails a waiting initial subscribe call, even
if recovery has started. Recovery then continues only while another consumer remains. A later
subscribe starts afresh, and other markets recover independently.

See [Order book recovery ownership](../developer_guide/adapters.md#order-book-recovery-ownership)
for the shared recovery machinery and adapter responsibilities.

### Live recovery validation

The `lighter-book-stress` harness is a development tool for changes to book synchronization and
recovery. It uses Lighter mainnet public market data, submits no orders, and checks six perpetual
books against the book stream contract, including rising nonces within each snapshot episode, and
against an independent reconstruction of the venue feed's best 20 levels.

From the repository root, run:

```bash
CARGO_BUILD_JOBS=16 bash scripts/strip-adapter-env.bash \
  cargo test -p nautilus-lighter --features examples --test lighter-book-stress -- --timeout 10 --rounds 12
```

`--scenario` selects the run:

- `churn` (default): checks recovery without reconnects, then rotates nonce gaps, dropped and
  delayed snapshots, rejected replacements, reconnects, and a restart during recovery.
- `initial`: drops each book's first snapshot, in a fresh session per round.
- `boundaries`: rejects every attempt in the retry budget, then checks the retry ceiling, a
  reconnect that ends the ceiling wait, unsubscribe during recovery, and shutdown during a reconnect.

`--timeout` sets the snapshot timeout in seconds (`0` disables deadlines), and
`--rounds` sets the number of rounds (12 by default).

The harness requires the mainnet WebSocket stream and the public `orderBooks` and `orderBookDetails`
APIs. See [Stress harnesses](../developer_guide/spec_data_testing.md#stress-harnesses) for the shared
flags and output format.

## Order capabilities

### Order identification

Lighter uses a numeric venue order index and a caller-supplied `client_order_index`. The adapter
derives a 31-bit client index from the Nautilus `ClientOrderId` and probes forward on collision.
After a restart, it cannot re-derive a probed value. Reconciliation therefore resolves each raw
venue order ID through the core cache and restores its actual `client_order_index` before
translating order and fill reports. Open cached orders return to active tracking; terminal orders
use bounded replay tracking.

Recovery never infers a client order ID from the integer alone. Reconciliation must include the
order, and the core cache must retain a matching venue-order-ID mapping. Otherwise, reports use the
unique venue order ID as their external client order ID.

Queries use the numeric venue order ID for active and terminal history. Before that ID is known,
the derived client index can query active orders only. Duplicate active matches fail as ambiguous.

### Order types

| Order type             | Perpetuals | Spot | Notes                                                   |
| ---------------------- | ---------- | ---- | ------------------------------------------------------- |
| `MARKET`               | ✓          | ✓    | Cap derived from cached far-side quote + slippage.      |
| `LIMIT`                | ✓          | ✓    | Requires a limit price.                                 |
| `STOP_MARKET`          | ✓          | -    | Perp only; cap derived from `trigger_price` + slippage. |
| `STOP_LIMIT`           | ✓          | -    | Perp only; maps to Lighter stop-loss limit orders.      |
| `MARKET_IF_TOUCHED`    | ✓          | -    | Perp only; cap derived from `trigger_price` + slippage. |
| `LIMIT_IF_TOUCHED`     | ✓          | -    | Perp only; maps to Lighter take-profit limit orders.    |
| `MARKET_TO_LIMIT`      | -          | -    | *Not supported*.                                        |
| `TRAILING_STOP_MARKET` | -          | -    | *Not supported*.                                        |
| `TRAILING_STOP_LIMIT`  | -          | -    | *Not supported*.                                        |
| `TWAP`                 | -          | -    | *Not supported*; no Nautilus mapping.                   |

Every conditional order requires `trigger_price`. The adapter rejects missing triggers, triggers
that truncate to `0` ticks at the instrument's price precision, and all spot conditional orders.

Lighter requires a worst-acceptable `price` for market-style orders. The adapter uses the cached
ask for a `MARKET` buy, the cached bid for a `MARKET` sell, or `trigger_price` for `STOP_MARKET`
and `MARKET_IF_TOUCHED`. It applies `market_order_slippage_bps` (default 50 bps), then rounds at
the instrument's price precision: up for buys, down for sells. A `MARKET` order without a cached
`QuoteTick` is denied. Override slippage with `SubmitOrder.params["market_order_slippage_bps"]`.

### Contingent orders

| Feature               | Perpetuals | Spot | Notes                                              |
| --------------------- | ---------- | ---- | -------------------------------------------------- |
| Stop-loss market      | ✓          | -    | `STOP_MARKET` maps to Lighter `STOP_LOSS`.         |
| Stop-loss limit       | ✓          | -    | `STOP_LIMIT` maps to Lighter `STOP_LOSS_LIMIT`.    |
| Take-profit market    | ✓          | -    | `MARKET_IF_TOUCHED` maps to Lighter `TAKE_PROFIT`. |
| Take-profit limit     | ✓          | -    | `LIMIT_IF_TOUCHED` maps to `TAKE_PROFIT_LIMIT`.    |
| Trigger price         | ✓          | -    | Required for every supported conditional order.    |
| Trigger price type    | -          | -    | *Not supported*; no trigger source selector.       |
| Grouped order lists   | -          | -    | *Not supported*.                                   |
| OCO / OTO orders      | -          | -    | *Not supported*.                                   |
| Bracket orders        | -          | -    | *Not supported*.                                   |
| `CreateGroupedOrders` | -          | -    | *Not supported*; order lists use independent txs.  |

### Order options

| Option           | Perpetuals | Spot | Notes                                                                     |
| ---------------- | ---------- | ---- | ------------------------------------------------------------------------- |
| `post_only`      | ✓          | ✓    | Maps to Lighter's post-only time-in-force.                                |
| `reduce_only`    | ✓          | -    | Passed through to `CreateOrder`; use only to reduce an existing position. |
| `quote_quantity` | -          | -    | *Not supported*; submit base quantity instead.                            |
| `display_qty`    | -          | -    | *Not supported*; Lighter exposes no iceberg display quantity field.       |

### Adapter order params

| Param                                      | Perpetuals | Spot | Notes                                               |
| ------------------------------------------ | ---------- | ---- | --------------------------------------------------- |
| `market_order_slippage_bps`                | ✓          | ✓    | Overrides the config default for market-style caps. |
| `post_only` through `SubmitOrder.params`   | -          | -    | *Not supported*; use the Nautilus order flag.       |
| `reduce_only` through `SubmitOrder.params` | -          | -    | *Not supported*; use the Nautilus order flag.       |

### Time in force

| Time in force  | Perpetuals | Spot | Notes                                                                         |
| -------------- | ---------- | ---- | ----------------------------------------------------------------------------- |
| `GTC`          | ✓          | ✓    | Limit-style uses `GoodTillTime`; market-style uses `IOC`.                     |
| `DAY`          | ✓          | ✓    | Limit-style and conditional orders use a positive order expiry.               |
| `GTD`          | ✓          | ✓    | Native expiry is 5 minutes to 30 days; see the managed-GTD policy below.      |
| `IOC`          | ✓          | ✓    | Plain `MARKET`/`LIMIT` use expiry `0`; conditional limit uses trigger expiry. |
| `FOK`          | -          | -    | *Not supported*.                                                              |
| `AT_THE_OPEN`  | -          | -    | *Not supported*.                                                              |
| `AT_THE_CLOSE` | -          | -    | *Not supported*.                                                              |

The adapter sends `MARKET`, `STOP_MARKET`, and `MARKET_IF_TOUCHED` as Lighter
`ImmediateOrCancel`; the venue rejects market-style `GoodTillTime` orders. Plain `MARKET` uses
`OrderExpiry = 0`, while conditional market orders keep a positive expiry until triggered.
The adapter denies Nautilus `IOC` for conditional market orders because Lighter reserves IOC for
post-trigger execution. Conditional limit orders can use `IOC`: their trigger rests with a positive
expiry, then the child uses `ImmediateOrCancel`.

Without an explicit GTD expiry, limit-style `GTC`, `DAY`, and `GTD` orders use the current time
plus 28 days. Conditional `GTC`, `DAY`, and limit-style `IOC` use the same default because the
venue has rejected `-1` in these paths with `21711 invalid expiry`.

#### GTD policy

With the default `use_gtd=True`, the strategy's `expire_time` becomes the venue `GoodTillTime`
expiry. The adapter accepts lifetimes in `[5 minutes + 1 second, 30 days]`; the extra second allows
for signing and transport.

For shorter lifetimes, set `use_gtd=False` only when the submitting strategy has
`manage_gtd_expiry=True`. Lighter has no `GoodTillCancel` time-in-force, so this setting cannot
switch the wire time-in-force as it does on Binance. The order still rests as `GoodTillTime` with
a 28-day fallback expiry, and the strategy's local GTD manager sends a cancellation at the strategy
expiry. Without that manager, the order can rest until the fallback expiry. This mode skips the native
5-minute minimum but rejects strategy expiries beyond 28 days because the venue would expire the
order first. Use native GTD for longer lifetimes. Venue cancel latency still delays removal of a
locally managed order.

### Execution instructions

| Instruction   | Perpetuals | Spot | Notes                                                     |
| ------------- | ---------- | ---- | --------------------------------------------------------- |
| `post_only`   | ✓          | ✓    | Overrides the TIF and sends Lighter `PostOnly`.           |
| `reduce_only` | ✓          | -    | Position-reducing flag for existing derivative positions. |

Use `post_only` on limit-style orders. The adapter does not synthesize maker-only market orders.
Live Lighter Mainnet testing confirms `reduce_only=true` for closing perpetual positions. Invalid
reduce-only opens can be dropped by Lighter without a venue order report; the adapter reconciles
them as `INFLIGHT_TIMEOUT` rather than a venue-supplied rejection reason.

### Advanced order features

| Feature            | Perpetuals | Spot | Notes                                                      |
| ------------------ | ---------- | ---- | ---------------------------------------------------------- |
| Order modification | ✓          | ✓    | Modify quantity, price, and trigger price on a live order. |
| Bracket orders     | -          | -    | *Not supported*.                                           |
| Iceberg orders     | -          | -    | *Not supported*.                                           |
| Trailing stops     | -          | -    | *Not supported*.                                           |
| Pegged orders      | -          | -    | *Not supported*.                                           |
| TWAP orders        | -          | -    | *Not supported*; no Nautilus mapping.                      |
| Leverage update    | ✓          | -    | Perp only; submits a signed `UpdateLeverage` tx.           |
| Native cancel-all  | -          | -    | *Not supported*; adapter scopes cancel-all per instrument. |
| Dead man's switch  | -          | -    | *Not supported*.                                           |

### Order operations

| Operation           | Perpetuals | Spot | Notes                                                          |
| ------------------- | ---------- | ---- | -------------------------------------------------------------- |
| Submit order        | ✓          | ✓    | Sends a signed `L2CreateOrder` transaction over WebSocket.     |
| Submit order list   | ✓          | ✓    | Sequential fanout of up to 15 independent create transactions. |
| Modify order        | ✓          | ✓    | Sends a signed `ModifyOrder`; reports may restate accepts.     |
| Cancel order        | ✓          | ✓    | Sends a signed `L2CancelOrder` transaction.                    |
| Cancel all orders   | ✓          | ✓    | Cancels cached orders by instrument and optional side.         |
| Set leverage        | ✓          | -    | Perp only; submits a signed `UpdateLeverage` tx.               |
| Batch cancel orders | ✓          | ✓    | WebSocket batches of up to 15 signed cancel transactions.      |
| Query order         | ✓          | ✓    | Requires credentials and REST lookup.                          |
| Query account       | ✓          | ✓    | Replays the latest private WebSocket account state.            |
| Mass status         | ✓          | ✓    | Bounded to account-active markets from WS and REST reports.    |

#### Order lists and batch cancellations

`SubmitOrderList` signs and sends each child transaction in order through the hash-correlated
WebSocket `sendTx` path, allocating each nonce after the prior handoff completes.

`BatchCancelOrders` uses WebSocket `jsonapi/sendtxbatch` with sequential nonces from the same API key.
`CancelAllOrders` chunks selected orders into batches of up to 15, preserving any side filter.
Explicit `BatchCancelOrders` requests above 15 orders are rejected. Cancelling 45 orders normally
produces three batches, but concurrent nonce allocation can make them smaller. Venue
[active-order limits](#active-and-pending-order-limits) still apply: Standard accounts cannot hold
45 active orders on one market.

Each batch uses one account and API key with consecutive nonces, as required by Lighter's
[nonce contract](https://apidocs.lighter.xyz/docs/core-concepts#nonce). The adapter uses
`skip_nonce=0`, so it must preserve nonce order. Its local window permits 16 unconfirmed nonce
allocations per key; this is an adapter capacity limit, not a venue allowance for out-of-order
transactions. Unsigned cancellations wait for capacity or nonce recovery instead of being discarded.
Shutdown stops new dispatch and drops any remaining queued work. Batch responses correlate through
the request ID and each signed transaction hash.

A pre-admission rejection fails the whole batch without consuming its nonces; an invalid-nonce
response also triggers nonce refresh. An acknowledgement confirms transaction admission, not order
cancellation. Individual cancellations can fail after admission while other transactions in the batch
succeed; these execution failures consume their nonces. Order updates and transaction lookups
resolve cancellation outcomes.
Ambiguous delivery retains pending state for reconciliation instead of resending the batch.
Before signing the next chunk, the adapter waits up to 10 seconds for the current chunk's
acknowledgements. On timeout, dispatch continues and retains unacknowledged entries for reconciliation.
Order lists and batch cancellations provide no atomic execution, grouped orders, OCO/OTO, or bracket semantics.

#### Leverage updates and signing validation

`UpdateLeverage` is exposed as `LighterExecutionClient::update_leverage(instrument_id,
initial_margin_fraction, margin_mode)`. The `initial_margin_fraction` is in venue ticks
(1e-4 fraction): `500` is 5% initial margin (20x leverage), `1000` is 10% (10x), and so on.

`UpdateLeverage`, `CancelAllOrders`, modify orders with integrator attributes, and conditional
create orders are byte-pinned against the signer distributed with the official `lighter-python`
SDK version 1.1.4.

### Order querying and reconciliation

| Feature              | Perpetuals | Spot | Notes                                                        |
| -------------------- | ---------- | ---- | ------------------------------------------------------------ |
| Query open orders    | ✓          | ✓    | REST `accountActiveOrders` scoped by market.                 |
| Query order history  | ✓          | ✓    | REST `accountInactiveOrders` with cursor pagination.         |
| Order status updates | ✓          | ✓    | Private WebSocket order streams plus status reports.         |
| Trade history        | ✓          | ✓    | REST `trades`; credentials are required for account history. |
| Fill reports         | ✓          | ✓    | REST and private WebSocket trade payloads.                   |
| Position reports     | ✓          | -    | Perp only; replays cached position stream.                   |
| Account state        | ✓          | ✓    | Replays the cached merged account state snapshot.            |
| Mass status          | ✓          | ✓    | Combines orders, fills, and cached positions.                |

Authenticated inactive-order and fill pagination rejects repeated cursors and stops after 1,000
pages. Fill reconciliation remains repeatable across calls while suppressing fills already emitted
from the live WebSocket stream. Historical order and fill reports bind a mapped client index only
to its matching venue order ID so reused numeric indexes cannot merge unrelated lifecycles.

#### Report completeness

Each bounded mass status uses one cutoff for inactive orders and fills. It is complete only when
all required order, fill, and position sources succeed and every historical fill maps to its order.
If history fails, active orders and explicit position reports remain available for reconciliation;
incompleteness does not veto a position report. Bounded historical fills without an in-scope position
report follow the engine's
[order-only projection](../concepts/execution/reconciliation.md#order-only-fill-projection) rules.

The `trades` endpoint retains only the most recent 3,000 trades per `account_index`, so a bounded
lookback can request more fill history than the venue serves. Pagination walks back from the newest
trade, and only a trade older than the lookback start proves the window was served:

- Trade older than the start: the report set stays complete.
- Cursor exhausted first: the adapter logs the uncovered span and marks the report set incomplete.
- No retained trades: nothing can have been truncated, so the report set stays complete.

Cursor exhaustion cannot distinguish truncation from an account with no older trades. A young
account can therefore report incomplete with nothing missing. Choose a lookback the venue can serve.
For older fills, the venue's [historical data exports](https://apidocs.lighter.xyz/docs/historical-data)
provide up to 12 months of account trades. The adapter does not read the `export` endpoint.

#### Startup position checks

Opening a position at strategy startup can trigger a transient warning (`cached=0, venue=N`) if
`account_all_positions` arrives milliseconds before the matching fill is processed. Applying the
fill resolves the discrepancy; no reconciliation orders are generated.

## Account and position management

Authenticated execution clients subscribe to these private streams:

- `account_all_orders`: order status reports.
- `account_all_trades`: fill reports.
- `account_all_positions`: initial position snapshot and live updates.
- `account_all_assets`: per-asset balance snapshots (spot balance plus perp collateral).
- `user_stats`: perp-account margin rollup (collateral and available balance).

The adapter merges `account_all_assets` and `user_stats` into a single account state and emits it
only after both streams have delivered their first frame.

You can construct an execution client without credentials, but `connect()` requires `private_key`,
`account_index`, and `api_key_index` to resolve. Private account streams and nonce refresh are mandatory.

### Position snapshots and updates

Perpetual positions use netting, with one position per market; spot balances use account asset state.
The authoritative `subscribed/account_all_positions` snapshot flattens omitted markets and rows
with zero `position`; an empty `positions` map flattens the entire cache. Unmapped or unparsable
rows retain their cached positions to prevent false flat reports.

For bounded reconciliation, the adapter records the current snapshot's market coverage; reconnect
invalidates it. An absent touched market produces an explicit flat report only after a current
snapshot covers it. Unmapped or malformed rows leave mass status incomplete instead of proving flat.

Incremental `update/account_all_positions` frames replace non-zero positions and flatten explicit
zero rows. Omitted markets remain cached, and an empty update retains all positions.

| Feature                 | Perpetuals | Spot | Notes                                                        |
| ----------------------- | ---------- | ---- | ------------------------------------------------------------ |
| Account balances        | ✓          | ✓    | Merged assets + `user_stats`, replayed from cache on query.  |
| Position state          | ✓          | -    | Perp only; initial snapshot plus live updates.               |
| Netting positions       | ✓          | -    | One Nautilus position per perpetual market.                  |
| Cross margin            | ✓          | -    | Passed through `LighterPositionMarginMode::Cross`.           |
| Isolated margin         | ✓          | -    | Passed through `LighterPositionMarginMode::Isolated`.        |
| Leverage updates        | ✓          | -    | Signed `UpdateLeverage` transaction.                         |
| Spot margin / borrowing | -          | -    | *Not supported*.                                             |
| Deposits / withdrawals  | -          | -    | Use venue tools or Lighter APIs outside the trading adapter. |

## Liquidation and ADL handling

| Event or field              | Support | Notes                                                         |
| --------------------------- | ------- | ------------------------------------------------------------- |
| Liquidation trades          | ✓       | Account trade rows can parse as fills, with no special event. |
| Deleverage trades           | ✓       | Account trade rows can parse as fills, with no special event. |
| Liquidation price reporting | -       | *Not supported*; reports omit this field.                     |
| ADL event stream            | -       | *Not supported*.                                              |

## Funding rates

Perpetual `market_stats` frames emit `MarkPriceUpdate`, `IndexPriceUpdate`, and
`FundingRateUpdate`. The live funding update uses `current_funding_rate` as the upcoming estimate;
`funding_rate` and `funding_timestamp` describe the last completed payment. Because market stats
provide no future settlement time, live updates leave `interval` and `next_funding_ns` unset. Spot
`spot_market_stats` frames emit `IndexPriceUpdate`.

Historical requests use public `/api/v1/fundings` rows at `1h` resolution and set `interval=60`.
`direction=long` stays positive, while `short` becomes negative. Pagination covers the requested
range up to the adapter's page cap, subject to an explicit `limit`; see
[Rate limiting](#rate-limiting). Account-specific `positionFunding` is not used.

## Account tiers

Account tiers set latency, rate limits, and trading fees. The client reads and logs the tier from
`GET /api/v1/account`, including unknown `account_type` values. [Zero-fee integrator attribution](#integrator-attribution)
applies to all Lighter Mainnet tiers. The client never raises quotas automatically; local overrides
do not grant higher venue limits.

The following figures apply to [Lighter Mainnet account tiers](https://apidocs.lighter.xyz/docs/account-types).

| Tier     | Latency (maker / taker) | REST weighted limit | `sendTx` limit          | Fees (maker / taker)            | Notes                                   |
| -------- | ----------------------- | ------------------- | ----------------------- | ------------------------------- | --------------------------------------- |
| Standard | 0 ms / 300 ms           | 60 req/min          | 60 req/min              | 0 / 0                           | Zero-fee default tier.                  |
| Premium  | 0 ms / 140 ms           | 24,000 req/min      | [4,000, 57,600] req/min | [0.28, 0.40] / [1.96, 2.80] bps | Lowest latency; scales with staked LIT. |
| Plus     | 0 ms / 300 ms           | 24,000 req/min      | 4,000 req/min           | 0.5 / 0.5 bps                   | Raised limits, standard latency.        |
| Builder  | -                       | 240,000 req/min     | -                       | -                               | Highest REST throughput.                |

Premium limits and fees scale with staked LIT. Robinhood uses different fees and limits, with
Premium tiers based on 14-day trading volume; see [Robinhood account tiers](https://apidocs.lighter.xyz/docs/lighter-rh#account-tiers).
Before raising a local quota, confirm the venue limit for the deployment and client traffic, then
set the quota explicitly (see [Rate limiting](#rate-limiting)).

## Rate limiting

Lighter limits both IP and L1 addresses. Each data and execution client owns a separate REST
limiter and defaults to the standard-account quota. Configure their combined traffic within the
venue limit.

Higher [account tiers](#account-tiers) still require explicit client quotas:

- `rest_quota_per_min`: REST read-bucket quota in requests per minute. Unset keeps 60 req/min.
  Available on both the data and execution clients.
- `sendtx_quota_per_min`: transaction quota in requests per minute, metered in a bucket separate
  from reads. Unset keeps it at the standard 60 req/min, independent of `rest_quota_per_min`.
  Execution client only.

These options change local pacing only. Public data requests remain unauthenticated, so setting a
higher local quota does not make those requests eligible for an account-level venue limit.

### Request buckets and endpoint weights

The venue meters transaction requests per L1 address across HTTP and WebSocket in one bucket.
A WebSocket `sendTxBatch` request counts once and carries up to 15 transactions. Standard accounts
share a 60-request-per-minute budget for reads and transactions; Plus and Premium have separate
venue buckets. The adapter uses separate local read and transaction limiters on every tier, so
Standard users must budget their combined traffic, including nonce and reconciliation reads.

The REST limiter counts one token per call rather than venue endpoint weights. Set
`rest_quota_per_min` for the endpoint mix: a 24,000 weighted req/min Premium limit permits
40 `/api/v1/recentTrades` calls/minute (weight 600) or 120 `/api/v1/trades` calls/minute
(weight 200) before other requests. These figures apply to Lighter Mainnet; confirm weights for
the selected deployment.

### Transaction type limits

Lighter documents a default transaction-type limit of 40 requests per minute, with exceptions by
transaction type. This is separate from the account-tier `sendTx`/`sendTxBatch` request limit.
A Lighter Mainnet quoting session amending on every quote drift hit
`code=23000` (`Too Many Requests`) after roughly 40 modify transactions in a minute; see
[Volume quota and no-fill quoting](#volume-quota-and-no-fill-quoting) for the related quota that
modify transactions also spend. Set `sendtx_quota_per_min` to 40 or lower for transaction-heavy
quoting workloads. The limiter is shared across all `sendTx` traffic, so a lower quota also paces
creates and cancels.

The local quotas allow bursts; they do not enforce a strict rolling-minute ceiling. A quota of
30 permits an initial burst of 30 requests while replenishing capacity at 30 per minute, so it can
still exceed a venue limit of 40 requests in 60 seconds. Leave headroom for that burst and for other
clients sharing the L1 address. For bounded Standard-account testing, 15 transaction requests and
5 REST calls per minute leave room for independent account checks.

An uncorrelated WebSocket `23000` response is logged without rejecting a particular transaction.
It can apply to non-transaction traffic, so the adapter does not guess which order or batch failed.
Pending outcomes require reconciliation; do not resend them blindly.

### Shared adapter limiters

The execution client enforces `sendtx_quota_per_min` with a single shared limiter across WebSocket `sendTx`
and `sendTxBatch` (including order lists and cancellation batches), and the HTTP `sendTx` used for
startup integrator approval. Low-level raw `sendTx` and `sendTxBatch` calls use that limiter when the client is
constructed with it; otherwise, they fall back to the raw client's REST limiter.

The clients share one WebSocket message limiter per venue URL. It paces non-transaction control
frames at 200 messages/minute across both clients. A closed-loop subscription gate caps
unacknowledged requests at 35, below the venue's 50-message per-IP ceiling; this count depends on
acknowledgement latency, not send rate. `sendTx` and `sendTxBatch` do not count against the
client-message bucket or its 50-message inflight cap.

The tier limits below apply to [Lighter Mainnet](https://apidocs.lighter.xyz/docs/rate-limits).

| Scope                                | Venue limit              | Adapter behavior                                     |
| ------------------------------------ | ------------------------ | ---------------------------------------------------- |
| REST, standard account               | 60 req/min               | Default; set `rest_quota_per_min` to override.       |
| REST, premium account                | 24,000 weighted req/min  | Local override required; venue attribution applies.  |
| REST, plus account                   | 24,000 weighted req/min  | Local override required; venue attribution applies.  |
| REST, builder account                | 240,000 weighted req/min | Local override required; venue attribution applies.  |
| `sendTx` / `sendTxBatch`, standard   | 60 req/min               | Shared with REST reads; includes HTTP and WebSocket. |
| `sendTx` / `sendTxBatch`, premium    | [4,000, 57,600] req/min  | Set `sendtx_quota_per_min` (scales with staked LIT). |
| `sendTx` / `sendTxBatch`, plus       | 4,000 req/min            | Set `sendtx_quota_per_min` to use it.                |
| Default transaction type limit       | 40 req/min               | Applies to tx types not covered by volume quota.     |
| `L2UpdateLeverage` transaction limit | 40 req/min               | Relevant to `update_leverage`.                       |

### Active and pending order limits

Lighter's [rate-limit documentation](https://apidocs.lighter.xyz/docs/rate-limits) specifies these
limits by account tier. Each per-market cap applies within the account.

| Account tier | Active per account | Active per market | Pending per account | Pending per market |
| ------------ | ------------------ | ----------------- | ------------------- | ------------------ |
| Standard     | 250                | 30                | 50                  | 10                 |
| Plus         | 750                | 250               | 500                 | 100                |
| Premium      | 1,500              | 1,000             | 1,000               | 100                |

Active orders rest on the book. Venue-pending orders include untriggered take-profit, stop-loss,
and TWAP orders, separate from Nautilus `PendingCancel` and unacknowledged transaction requests.
The adapter does not pre-count these limits. Leave room for existing orders; raising local quotas
does not raise venue caps.

### Endpoint weights and transport limits

Common REST weights from the [Lighter Mainnet rate limits](https://apidocs.lighter.xyz/docs/rate-limits):

| Endpoint group                                                  | Weight | Adapter behavior                                |
| --------------------------------------------------------------- | ------ | ----------------------------------------------- |
| `sendTx`, `sendTxBatch`, `nextNonce`                            | 6      | Tx calls use tx limiter; `nextNonce` uses REST. |
| `accountInactiveOrders`, `accountActiveOrders`, `accountOrders` | 100    | Adapter counts one REST token per HTTP call.    |
| `apikeys`                                                       | 150    | Adapter counts one REST token per HTTP call.    |
| `trades`                                                        | 200    | Adapter counts one REST token per HTTP call.    |
| `recentTrades`                                                  | 600    | Adapter counts one REST token per HTTP call.    |
| Other endpoints in the official table                           | 300    | Adapter counts one REST token per HTTP call.    |

Other named endpoints have distinct weights. Check the official table before budgeting calls.
Robinhood weights can differ; see [Robinhood rate limits](https://apidocs.lighter.xyz/docs/lighter-rh#rate-limits).

| Endpoint or transport                  | Limit      | Notes                                                      |
| -------------------------------------- | ---------- | ---------------------------------------------------------- |
| `/api/v1/trades`                       | 100 rows   | Adapter paginates reconciliation at this cap.              |
| `/api/v1/accountInactiveOrders`        | 100 rows   | Adapter follows `next_cursor` at this cap.                 |
| `/api/v1/orderBookOrders`              | 250 levels | Snapshot depth is clamped to the venue cap.                |
| `/api/v1/candles`                      | 500 rows   | Adapter caps REST bar pages at this venue maximum.         |
| `/api/v1/fundings`                     | 750 rows   | Venue cap; adapter requests 100 rows per page.             |
| WebSocket connections                  | 255 / IP   | Venue limit.                                               |
| WebSocket subscriptions / connection   | 500        | Venue limit.                                               |
| WebSocket subscriptions / IP           | 5,000      | Venue limit.                                               |
| WebSocket unique accounts / connection | 500        | Venue limit.                                               |
| WebSocket unique accounts / IP         | 5,000      | Venue limit.                                               |
| WebSocket connections / minute         | 255        | Venue limit.                                               |
| WebSocket client messages / minute     | 200        | Paces non-tx frames; heartbeat pings bypass it.            |
| WebSocket inflight messages            | 50         | Venue cap; subscriptions use a 35-frame closed loop.       |
| WebSocket `sendTxBatch` batch size     | 15 txs     | Cancel-all chunks; explicit batches above 15 are rejected. |
| WebSocket keepalive                    | 2 minutes  | Adapter sends heartbeats every 30 seconds.                 |
| WebSocket outbound command queue       | Not capped | Paced before writes; no queue-depth cap.                   |

### Historical request limits

Bar and funding history stop after 500 REST pages, covering up to 250,000 bars or 49,500 hourly
funding intervals. An uncovered range returns `LighterHttpError::HistoryIncomplete`, without partial
history or retry. Completion on the final page succeeds, as does a request with explicit `start`
that satisfies its explicit `limit`. The data client logs incomplete history and emits no response;
narrow the range to continue.

## Volume quota and no-fill quoting

Volume quota applies only to Plus and Premium accounts and is separate from transport limits.
`L2CreateOrder`, `L2CancelAllOrders`, `L2ModifyOrder`, and `L2CreateGroupedOrders` spend it;
completed trading volume and the free allowance replenish it. Each eligible transaction in a batch
spends quota separately; single-order cancellations do not. The adapter does not inspect remaining
quota. See Lighter's
[Volume Quota](https://apidocs.lighter.xyz/docs/volume-quota-program) documentation for current
rules and figures.

Repeated no-fill quote refreshes can exhaust this quota even when the WebSocket and `sendTx`
limiters work. For live tests, prefer slower one-sided quoting, wider refresh thresholds, testnet,
or a bounded strategy that earns enough fills to replenish its quota.

## Connection management

### Heartbeats and reconnects

The WebSocket client sends heartbeats every 30 seconds and reconnects with exponential backoff in
[250 milliseconds, 30 seconds]. After 90 seconds without any inbound frame, it reconnects to
recover stalled sockets even if the venue leaves them open. Heartbeat pongs refresh this window
on healthy connections even when no market data flows.

### Private authentication

Private subscriptions use auth tokens with an 8-hour maximum lifetime. The adapter mints 7-hour
tokens, rotates them every 6 hours, and resubscribes. A transparent reconnect triggers a fresh token
and account resubscription after tracked subscriptions start replaying.

### Nonce recovery

On execution reconnect, the adapter starts a nonce-baseline refresh through
`GET /api/v1/nextNonce`. Submit, modify, and single-cancel commands cannot sign until that refresh,
or its background retry, installs the replacement connection's nonce baseline. Batch cancellations
wait for nonce readiness, including requests received during the refresh.

Within a session, venue confirmations advance the local nonce window, definitive rejections or
pre-handoff failures may roll back its latest nonce, and stale state triggers a
`GET /api/v1/nextNonce` resync. Outcomes that may have reached the venue retain their pending nonce
and order identity for WebSocket or reconciliation recovery.

### Account stream readiness

`LighterExecutionClient::connect()` waits up to 30 seconds for every account stream
(`account_all_orders`, `account_all_trades`, `account_all_positions`, `account_all_assets`,
`user_stats`) to satisfy its readiness condition. For positions, only the
`subscribed/account_all_positions` snapshot satisfies this wait; a live update does not. The adapter
does not use REST account payloads as a fallback. Each attempt clears old position and account
caches before awaiting the session's frames.
Transparent WebSocket reconnects and auth-token rotations do not re-enter `connect()`. Both retain
cached positions until the next `subscribed/account_all_positions` frame applies the snapshot
replacement rules. Live update frames merge into the retained cache without evicting omitted
markets.

## API credentials

Lighter signing requires all three credential values:

- Account index: numeric Lighter account identifier.
- API key index: numeric API key slot. Use an unreserved index in `[4, 254]`; Robinhood also reserves
  `157`. Do not use `[0, 3]` or `255`; `255` is an `apikeys` query sentinel, not a signing key.
- API private key: 40-byte hex private key, with or without a `0x` prefix.

Config values take precedence. A missing config field, or a blank API private key (empty or
whitespace only), falls back to the corresponding environment variable selected by `deployment`
and `environment`.

| Deployment | Environment | API key index                             | API private key                        | Account index                             |
| ---------- | ----------- | ----------------------------------------- | -------------------------------------- | ----------------------------------------- |
| Lighter    | Mainnet     | `LIGHTER_API_KEY_INDEX`                   | `LIGHTER_API_SECRET`                   | `LIGHTER_ACCOUNT_INDEX`                   |
| Lighter    | Testnet     | `LIGHTER_TESTNET_API_KEY_INDEX`           | `LIGHTER_TESTNET_API_SECRET`           | `LIGHTER_TESTNET_ACCOUNT_INDEX`           |
| Robinhood  | Mainnet     | `LIGHTER_ROBINHOOD_API_KEY_INDEX`         | `LIGHTER_ROBINHOOD_API_SECRET`         | `LIGHTER_ROBINHOOD_ACCOUNT_INDEX`         |
| Robinhood  | Testnet     | `LIGHTER_ROBINHOOD_TESTNET_API_KEY_INDEX` | `LIGHTER_ROBINHOOD_TESTNET_API_SECRET` | `LIGHTER_ROBINHOOD_TESTNET_ACCOUNT_INDEX` |

The four namespaces let one process run clients for multiple deployment targets without sharing
credentials.

Execution rejects incomplete credentials. The data client runs without credentials: its
subscriptions and REST requests (instruments, book, trades, bars, funding) all use public
endpoints.

## Configuration

### Data client configuration options

| Option                             | Default   | Description                                                   |
| ---------------------------------- | --------- | ------------------------------------------------------------- |
| `environment`                      | `Mainnet` | `LighterEnvironment::Mainnet` or `Testnet`.                   |
| `deployment`                       | `Lighter` | `LighterDeployment::Lighter` or `Robinhood`.                  |
| `venue`                            | `None`    | Optional Nautilus venue override; defaults from `deployment`. |
| `account_index`                    | `None`    | Optional factory field; public data calls do not use it.      |
| `api_key_index`                    | `None`    | Optional factory field; public data calls do not use it.      |
| `private_key`                      | `None`    | Optional factory field; public data calls do not use it.      |
| `base_url_http`                    | `None`    | Optional REST URL override.                                   |
| `base_url_ws`                      | `None`    | Optional WebSocket URL override.                              |
| `proxy_url`                        | `None`    | Optional proxy URL for HTTP and WebSocket.                    |
| `http_timeout_secs`                | `60`      | HTTP request timeout in seconds.                              |
| `ws_timeout_secs`                  | `30`      | WebSocket connection and reconnection timeout.                |
| `update_instruments_interval_mins` | `60`      | Instrument metadata refresh interval in minutes.              |
| `book_snapshot_timeout_secs`       | `10`      | Initial, reconnect, and recovery snapshot wait.               |
| `rest_quota_per_min`               | `None`    | REST quota override; unset keeps 60 req/min.                  |
| `transport_backend`                | Default   | WebSocket transport backend.                                  |

### Execution client configuration options

| Option                      | Default       | Description                                                   |
| --------------------------- | ------------- | ------------------------------------------------------------- |
| `environment`               | `Mainnet`     | `LighterEnvironment::Mainnet` or `Testnet`.                   |
| `deployment`                | `Lighter`     | `LighterDeployment::Lighter` or `Robinhood`.                  |
| `venue`                     | `None`        | Optional Nautilus venue override; defaults from `deployment`. |
| `account_id`                | `LIGHTER-001` | Nautilus account ID; issuer must match the resolved venue.    |
| `account_index`             | `None`        | Lighter account index.                                        |
| `api_key_index`             | `None`        | Lighter API key slot.                                         |
| `private_key`               | `None`        | Hex private key for auth and L2 transaction signing.          |
| `base_url_http`             | `None`        | Optional REST URL override.                                   |
| `base_url_ws`               | `None`        | Optional WebSocket URL override.                              |
| `proxy_url`                 | `None`        | Optional proxy URL for HTTP and WebSocket.                    |
| `http_timeout_secs`         | `60`          | HTTP request timeout in seconds.                              |
| `ws_timeout_secs`           | `30`          | WebSocket connection and reconnection timeout.                |
| `market_order_slippage_bps` | `50`          | Slippage cap (bps) for `MARKET` / `STOP_MARKET` / `MIT`.      |
| `rest_quota_per_min`        | `None`        | REST quota override; unset keeps 60 req/min.                  |
| `sendtx_quota_per_min`      | `None`        | Transaction quota override; unset keeps 60 req/min.           |
| `transport_backend`         | Default       | WebSocket transport backend.                                  |
| `use_gtd`                   | `True`        | Use venue-native GTD; see [GTD policy](#gtd-policy).          |

### Configuration example

```rust
use nautilus_lighter::{
    common::enums::{LighterDeployment, LighterEnvironment},
    config::{LighterDataClientConfig, LighterExecutionClientConfig},
};
use nautilus_model::identifiers::AccountId;

let data_config = LighterDataClientConfig::builder()
    .environment(LighterEnvironment::Testnet)
    .deployment(LighterDeployment::Lighter)
    .build();

let exec_config = LighterExecutionClientConfig::builder()
    .environment(LighterEnvironment::Testnet)
    .deployment(LighterDeployment::Lighter)
    .account_id(AccountId::from("LIGHTER-001"))
    .build();

let robinhood_data_config = LighterDataClientConfig::builder()
    .environment(LighterEnvironment::Mainnet)
    .deployment(LighterDeployment::Robinhood)
    .build();

let robinhood_exec_config = LighterExecutionClientConfig::builder()
    .environment(LighterEnvironment::Mainnet)
    .deployment(LighterDeployment::Robinhood)
    .account_id(AccountId::from("LIGHTER_ROBINHOOD-001"))
    .build();
```

Each execution config resolves credentials from the environment-variable set selected by its
`deployment` and `environment`; set the credential fields directly to override them. Use
`LiveExecutionEngineConfig.reconciliation_instrument_ids` to scope reconciliation and
`reconciliation_lookback_mins` to bound inactive order and fill replay.

## Official documentation

- [Get started](https://apidocs.lighter.xyz/docs/get-started)
- [Trading and signing](https://apidocs.lighter.xyz/docs/trading)
- [Core concepts and nonces](https://apidocs.lighter.xyz/docs/core-concepts)
- [Environments and endpoints](https://apidocs.lighter.xyz/docs/environments)
- [Lighter on Robinhood Chain](https://apidocs.lighter.xyz/docs/lighter-rh)
- [API keys](https://apidocs.lighter.xyz/docs/api-keys)
- [Account types](https://apidocs.lighter.xyz/docs/account-types)
- [Rate limits](https://apidocs.lighter.xyz/docs/rate-limits)
- [Volume quota](https://apidocs.lighter.xyz/docs/volume-quota-program)
- [Historical data and exports](https://apidocs.lighter.xyz/docs/historical-data)
- [Data structures, constants, and errors](https://apidocs.lighter.xyz/docs/data-structures-constants-and-errors)
- [REST OpenAPI](https://raw.githubusercontent.com/elliottech/lighter-python/main/openapi.json)
- [WebSocket reference](https://apidocs.lighter.xyz/docs/websocket-reference)

## Contributing

:::info
For additional features or to contribute to the Lighter adapter, please see our
[contributing guide](https://github.com/nautechsystems/nautilus_trader/blob/develop/CONTRIBUTING.md).
:::
