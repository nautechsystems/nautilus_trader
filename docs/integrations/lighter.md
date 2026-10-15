# Lighter

[Lighter](https://lighter.xyz) is a decentralized central-limit-order-book exchange for spot and
perpetual futures, with Ethereum zero-knowledge rollup settlement and off-chain matching and
sequencing. The adapter supports both Lighter and its Robinhood Chain deployment.

The `nautilus-lighter` crate provides Rust data and execution clients, typed REST and WebSocket
models, and an in-tree L2 signer using Schnorr signatures over ECgFp5. This page covers deployment
configuration, capabilities, and operational limits.

For L2 signing benchmarks and the official Go SDK comparison, see
[`crates/adapters/lighter/benches/BENCHMARKS.md`](../../crates/adapters/lighter/benches/BENCHMARKS.md).
Absolute timings vary by machine, so only deltas measured on the same machine are meaningful.

## Overview

- `LighterRawHttpClient`: low-level REST client for the public and account endpoints.
- `LighterHttpClient`: domain REST client that parses instruments, trades, bars, funding, and books
  into Nautilus model types and returns typed venue order and account payloads.
- `LighterWebSocketClient`: reconnecting WebSocket client for public market and private account streams.
- `LighterDataClient`: Nautilus data client for instruments, trades, quotes, and L2 market-by-price (MBP) books.
- `LighterExecutionClient`: Nautilus execution client for account streams, order submission,
  modification, cancellation, and reconciliation reports.
- `LighterDataClientFactory` and `LighterExecutionClientFactory`: live-node factory wiring.

The Python extension exposes configuration, deployment and environment selection, factories, and
integrator revocation. Live nodes consume the data and execution clients through Rust traits.

## Examples

Before running a tester, edit its module-level constants, including `LIGHTER_DEPLOYMENT` and
`LIGHTER_ENVIRONMENT`. Rust and Python testers start and connect immediately. These selectors do not read
environment variables.

:::warning
Both execution testers enable order submission by default: Rust sets `DRY_RUN = false` and selects Lighter
Mainnet. Python sets `DRY_RUN = False` and selects Lighter Testnet. On either mainnet deployment, a funded
account can place real orders. Review the instrument, quantity, deployment, and environment before running a
tester, or enable `DRY_RUN` to connect without submitting orders.
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

The command does not confirm or retry requests. Successful exit means the cancellation and
discovered closes were submitted without a known error, not that the account is flat. Check account
state afterward and rerun if anything remains.

Each run submits at most 15 closes: cancellation uses one slot in the 16-transaction nonce window.
An incomplete snapshot, request failure, or submission failure returns an error. The close pass times out
after 15 minutes.

Set `LIGHTER_DEPLOYMENT` to `lighter` or `robinhood` and `LIGHTER_ENVIRONMENT` to `mainnet` or `testnet`.
Omitted selectors default to Lighter Mainnet and select the matching credential namespace.

## Product support

Capability matrices describe adapter support. *Not supported by Lighter* identifies a venue API limitation.
*Not currently implemented* identifies an adapter gap.

| Product type      | Data feed | Trading | Notes                                                     |
| ----------------- | --------- | ------- | --------------------------------------------------------- |
| Spot              | ✓         | ✓       | Spot markets; new listings use 64-bit ids from 4095.      |
| Perpetual futures | ✓         | ✓       | Linear perpetuals; new listings use 64-bit ids from 4095. |
| Dated futures     | -         | -       | *Not supported by Lighter*.                               |
| Options           | -         | -       | *Not supported by Lighter*.                               |

## Limitations

- Native OTO, OCO, and OTOCO groups require the shapes described under
  [Contingent orders](#contingent-orders). Nautilus OUO relationships are not supported by native groups. TWAP
  is not implemented. Lighter has no trailing-stop order type or iceberg display-size field.
- Non-contingent order lists, batch modifications, and explicit batch cancellations automatically split
  supported children into WebSocket batches of up to 15 transactions. Native contingent lists use
  one grouped transaction.
- `CancelAllOrders` cancels cached open orders across strategies on the client's account, preserving each
  order's strategy and filtering by instrument and optional `order_side`. It uses per-order cancellations in
  batches of up to 15. The adapter's native cancel-all schema is account-wide. Lighter's
  [newer signer](https://apidocs.lighter.xyz/docs/wasm-signer#signcancelallorders) adds a market filter but no
  side filter.
- Spot trading supports market and limit orders. Conditional stop-loss and take-profit orders are
  limited to perpetual markets.
- `query_account` and position reports replay the latest cached private WebSocket state.
- Unscoped order reconciliation is bounded to configured or observed active markets to avoid a full
  venue-wide fan-out under the standard REST quota.

## Symbology

Lighter uses 64-bit `market_index` values. Existing markets keep their range-partitioned IDs. Following
[Lighter Mainnet's September 2026 upgrade](https://t.me/lighter_api_updates/174) and
[Robinhood's later upgrade](https://t.me/lighter_api_updates/184), new spot and perpetual markets take the
next free index at or above `4095`.

Product type always comes from `market_type`, never from the index. The adapter loads the mapping
from `GET /api/v1/orderBookDetails` and converts venue symbols to Nautilus `InstrumentId` values.

| Deployment product  | Nautilus symbol format                  | Example                            | Notes                    |
| ------------------- | --------------------------------------- | ---------------------------------- | ------------------------ |
| Lighter perpetual   | `{BASE}-PERP.LIGHTER`                   | `BTC-PERP.LIGHTER`                 | Raw venue symbol `BTC`.  |
| Lighter spot        | `{BASE}/{QUOTE}-SPOT.LIGHTER`           | `ETH/USDC-SPOT.LIGHTER`            | Raw symbol `ETH/USDC`.   |
| Robinhood perpetual | `{BASE}-PERP.LIGHTER_ROBINHOOD`         | `SNDK-PERP.LIGHTER_ROBINHOOD`      | Raw venue symbol `SNDK`. |
| Robinhood spot      | `{BASE}/{QUOTE}-SPOT.LIGHTER_ROBINHOOD` | `SNDK/USDG-SPOT.LIGHTER_ROBINHOOD` | Raw symbol `SNDK/USDG`.  |

The suffix separates spot and perpetual listings. Outbound requests strip it and use the cached
`market_index`. Spot symbols retain the venue pair.

## Deployments and environments

| Deployment | Environment | REST URL                              | WebSocket URL                              | L2 signing chain ID | Settlement | Default venue       |
| ---------- | ----------- | ------------------------------------- | ------------------------------------------ | ------------------- | ---------- | ------------------- |
| Lighter    | Mainnet     | `https://mainnet.zklighter.elliot.ai` | `wss://mainnet.zklighter.elliot.ai/stream` | 304                 | USDC       | `LIGHTER`           |
| Lighter    | Testnet     | `https://testnet.zklighter.elliot.ai` | `wss://testnet.zklighter.elliot.ai/stream` | 300                 | USDC       | `LIGHTER`           |
| Robinhood  | Mainnet     | `https://api.rh.lighter.xyz`          | `wss://api.rh.lighter.xyz/stream`          | 466324              | USDG       | `LIGHTER_ROBINHOOD` |
| Robinhood  | Testnet     | `https://api.rh-testnet.lighter.xyz`  | `wss://api.rh-testnet.lighter.xyz/stream`  | 300                 | USDG       | `LIGHTER_ROBINHOOD` |

These chain IDs are Lighter L2 signing-domain values, not EVM network chain IDs.

Select `LighterDeployment::Lighter` or `Robinhood` and `LighterEnvironment::Mainnet` or `Testnet`.
Together, they set default URLs, chain ID, settlement currency, venue, and attribution policy.
Both testnets use chain ID 300, so the adapter never infers deployment behavior from the numeric ID.

URL overrides for private gateways or local test fixtures change only transport endpoints, leaving
deployment signing, settlement currency, and attribution unchanged.

The data client appends `readonly=true` to its WebSocket URL. The execution client leaves the URL unchanged.

### Custom venue identity

For distinct Nautilus identities across Lighter-protocol endpoints, set `venue` on both client
configs. It scopes instruments, cache entries, message topics, socket state, and execution routing
without changing deployment protocol behavior.

The factory name remains `LIGHTER` for compatibility. `ClientId` is the client registration name. For
Robinhood routing by `ClientId`, register as `LIGHTER_ROBINHOOD`. Both language examples derive it from
`LIGHTER_DEPLOYMENT`. Custom client names are supported.

Nautilus routes account commands by issuer, so the `account_id` issuer must match the resolved venue:
`LIGHTER_RH_ALT` requires an ID such as `LIGHTER_RH_ALT-001`. Custom venues do not enable custom
chain IDs or attribution.

## Account and API key setup

Public market data needs no account. Private streams and execution require an account index,
API key index, and private key from one deployment and environment. Each row has a separate namespace:

| Deployment | Environment | Account and API key page                                        | Account issuer      | Credential prefix             |
| ---------- | ----------- | --------------------------------------------------------------- | ------------------- | ----------------------------- |
| Lighter    | Mainnet     | [Lighter Mainnet](https://app.lighter.xyz/apikeys)              | `LIGHTER`           | `LIGHTER_*`                   |
| Lighter    | Testnet     | [Lighter Testnet](https://testnet.app.lighter.xyz/apikeys)      | `LIGHTER`           | `LIGHTER_TESTNET_*`           |
| Robinhood  | Mainnet     | [Robinhood Mainnet](https://robinhoodchain.lighter.xyz/apikeys) | `LIGHTER_ROBINHOOD` | `LIGHTER_ROBINHOOD_*`         |
| Robinhood  | Testnet     | [Robinhood Testnet](https://rhctestnet.lighter.xyz/apikeys)     | `LIGHTER_ROBINHOOD` | `LIGHTER_ROBINHOOD_TESTNET_*` |

Do not mix accounts or keys across rows, even between testnets with signing chain ID 300.

1. Open the target deployment's account page and sign in. Create or select the trading account,
   including the intended sub-account, before generating its API key.
1. Follow Lighter's
   [account-index lookup](https://apidocs.lighter.xyz/docs/get-started#find-your-account-index)
   against the deployment's REST URL. For a deployment other than Robinhood Mainnet, replace the
   example URL with its [deployment-table value](#deployments-and-environments):

   ```bash
   LIGHTER_SETUP_API_URL="https://api.rh.lighter.xyz"
   LIGHTER_SETUP_L1_ADDRESS="0xYOUR_ETHEREUM_ADDRESS"

   curl -sS --get \
     "${LIGHTER_SETUP_API_URL}/api/v1/accountsByL1Address" \
     --data-urlencode "l1_address=${LIGHTER_SETUP_L1_ADDRESS}"
   ```

   Read the `index` from the required entry in `sub_accounts`. A wallet can own a main account and
   several sub-accounts, each with a separate account index and API keys.
1. On the selected account's API key page, choose **Generate API Key**. Use an unused index in `[4, 254]`.
   Lighter's [API key documentation](https://apidocs.lighter.xyz/docs/api-keys) reserves `[0, 3]` for its
   interfaces, and `255` is an API query sentinel. Robinhood also reserves `157`. See
   [Robinhood API keys](https://apidocs.lighter.xyz/docs/lighter-rh#api-keys).
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

Success returns `"code": 200` and the selected key. This public lookup confirms the indexes. It neither
exposes nor validates the private key.

:::warning
Lighter API keys authorize trading, private account access, and some withdrawal operations. Store
the private key in a secret manager or protected environment configuration. Do not commit it to a
repository or share it in logs.
:::

## Integrator attribution

Through [Lighter's partner program](https://apidocs.lighter.xyz/docs/partner-integration), the
execution client attributes Lighter Mainnet create and modify transactions to NautilusTrader's
integrator account in `L2TxAttributes`. This applies to every tier, including Standard, and every
order in a list. Maker and taker integrator fees are zero.

On Lighter Mainnet, startup submits `ApproveIntegrator` for non-maker-only keys with an L2 signature
and all maximum fees set to zero. No L1 signature is required.

Lighter Testnet and both Robinhood environments leave `L2TxAttributes` empty and omit
`ApproveIntegrator` during startup.

Selecting Robinhood Mainnet opts the account into the `NAUTILUS` referral code. At startup, the
client uses the configured L2 API key to apply the code to the account's public L1 address.
Failures log a warning without blocking trading. Robinhood Testnet performs no referral attribution.

Attribution follows the typed deployment and environment, regardless of custom venue names.

Maker-only Lighter Mainnet keys cannot submit `ApproveIntegrator` and skip automatic approval.
Before they can trade through the adapter, a non-maker-only key on the same account must approve
the integrator: approval is account-scoped.

### Revoking the approval

To revoke Lighter Mainnet approval when leaving the adapter, send `ApproveIntegrator` with
`approval_expiry = 0` and zero maximum fees. The next execution-client startup with a non-maker-only
key renews zero-fee approval on every Lighter Mainnet tier.

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

The Rust command displays the action and waits for Enter before signing or sending. Abort with `Ctrl+C` if the
summary is wrong. The Python binding does not prompt. Review the active environment variables before calling
it.

## Data subscriptions

| Data type            | Sub.         | Snapshot | Hist. | Nautilus type       | Notes                                                 |
| -------------------- | ------------ | -------- | ----- | ------------------- | ----------------------------------------------------- |
| Instrument metadata  | Cache replay | ✓        | -     | `InstrumentAny`     | Loaded from `orderBookDetails`.                       |
| Trade ticks          | ✓            | -        | ✓     | `TradeTick`         | WebSocket trades; public `recentTrades` REST history. |
| Quote ticks          | ✓            | -        | -     | `QuoteTick`         | Live BBO; history *Not supported by Lighter*.         |
| Order book deltas    | ✓            | ✓        | -     | `OrderBookDeltas`   | `L2_MBP`; history *Not supported by Lighter*.         |
| Order book depth     | ✓            | -        | -     | `OrderBookDepth`    | Live top-10; REST *Not currently implemented*.        |
| Order book snapshots | -            | ✓        | -     | `OrderBook`         | REST snapshot; aggregates up to 250 orders per side.  |
| Mark prices          | ✓            | -        | -     | `MarkPriceUpdate`   | Perp market stats stream.                             |
| Index prices         | ✓            | -        | -     | `IndexPriceUpdate`  | Market and spot stats streams.                        |
| Funding rates        | ✓            | -        | ✓     | `FundingRateUpdate` | Current estimates and REST hourly history.            |
| Bars                 | ✓            | -        | ✓     | `Bar`               | WebSocket candle stream; REST history for backfill.   |
| Instrument status    | REST         | ✓        | -     | `InstrumentStatus`  | `active` / `inactive` snapshots.                      |

### Order book data

Book-delta and depth subscriptions reject types other than `BookType::L2_MBP` before subscribing.

Only `subscribed/order_book` initializes the WebSocket book. Until it arrives, the adapter drops
`update/order_book` frames and emits no book data because incrementals omit unchanged levels.

Depth and deltas share the WebSocket `order_book` stream. Each accepted snapshot or incremental
update produces a refreshed top-10 depth view.

### Bars

Bar subscriptions use `candle/{market_id}/{resolution}`. Lighter batches open-candle updates about
every 500 ms. The adapter emits one `Bar` per closed period, only when the candle start timestamp
advances. Reconnect and unsubscribe clear the in-progress cache.

REST and WebSocket support `1m`, `5m`, `15m`, `30m`, `1h`, `4h`, `12h`, and `1d`. The adapter accepts `1w` for
REST requests, but Lighter Mainnet rejects that resolution with `20001 invalid param`. The
[REST schema](https://apidocs.lighter.xyz/reference/candles) does not list it. Weekly subscriptions return an
error locally. Aggregate shorter bars to obtain weekly data.

REST history skips gap rows with missing, null, or nonpositive OHLC values: they cannot form valid
Nautilus bars. Later valid rows still load.

### Instrument status and trades

Instrument status subscriptions replay cached `orderBookDetails` status or fetch a REST snapshot.
Lighter exposes no WebSocket status-change stream.

Trade subscriptions use the public WebSocket stream. History uses public `/api/v1/recentTrades` without
credentials. The adapter filters at most 100 trades to the requested time range without pagination.

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

After the subscription write completes, initial and replacement subscriptions wait up to
`book_snapshot_timeout_secs` (default 10 seconds) for a typed `subscribed/order_book` snapshot.
Set it to `0` to disable snapshot deadlines.

A missing snapshot starts or retries recovery, even after a control acknowledgement or
`Already Subscribed` response. Control acknowledgements release subscription slots without
completing book recovery.

Book output resumes only after a matching snapshot replaces the cached levels. An empty snapshot
clears the book too.

### Retry limits and reconnects

Each recovery episode makes up to eight replacement attempts within 180 seconds using exponential
backoff and jitter. It then retries with exponential backoff in [1 minute, 15 minutes] until a
snapshot is accepted. Replacement unsubscribe and subscribe writes target the same connection.

Reconnect retires obsolete subscription generations while preserving recovery and its remaining
budget. Recovery that exhausted its budget and is waiting between attempts retries immediately on
the new connection.

### Consumers and persistent failures

Deltas and depth share a recovery episode for each market:

- Removing one consumer preserves the other.
- Removing the final consumer cancels pending writes and snapshot waits.
- Shutdown cancels all owned work.

Recovery never ends in a failed state. Rejected replacements or subscriptions replayed after reconnect keep
retrying at the growing interval. A late snapshot still restores the book.

A venue subscription failure other than rate limiting fails a waiting initial subscribe call, even
if recovery has started. Recovery then continues only while another consumer remains. A later
subscribe starts afresh, and other markets recover independently.

See [Order book recovery ownership](../developer_guide/adapters.md#order-book-recovery-ownership)
for the shared recovery machinery and adapter responsibilities.

### Live recovery validation

Use `lighter-book-stress` to validate synchronization and recovery changes on six Lighter mainnet perpetual
books. It submits no orders. It checks the book stream contract, including nonce increases within each
snapshot episode. It also compares against an independent reconstruction of the feed's best 20 levels.

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

Lighter uses a numeric venue order index and caller-supplied `client_order_index`. The adapter
derives a 31-bit index from Nautilus `ClientOrderId` and probes forward on collision.

Probed values cannot be re-derived after restart. Before translating order and fill reports, reconciliation
resolves each venue order ID through the core cache and restores its actual client index. Open cached orders
return to active tracking. Terminal orders use bounded replay tracking.

Recovery never infers a client order ID from the integer alone. Reconciliation must include the order and the
core cache must retain its venue-order-ID mapping. Otherwise, reports use the unique venue order ID as their
external client order ID.

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
| `MARKET_TO_LIMIT`      | -          | -    | *Not supported by Lighter*.                             |
| `TRAILING_STOP_MARKET` | -          | -    | *Not supported by Lighter*.                             |
| `TRAILING_STOP_LIMIT`  | -          | -    | *Not supported by Lighter*.                             |
| `TWAP`                 | -          | -    | *Not currently implemented*; no Nautilus mapping.       |

Conditional orders require `trigger_price`. The adapter rejects missing triggers, triggers that
truncate to `0` ticks at instrument price precision, and all spot conditional orders.

Lighter requires a worst-acceptable `price` for market-style orders. The cap starts from:

- `MARKET`: cached ask for buys, bid for sells. Without a cached `QuoteTick`, the order is denied.
- `STOP_MARKET` and `MARKET_IF_TOUCHED`: `trigger_price`.

The adapter applies `market_order_slippage_bps` (default 50 bps), then rounds to instrument price
precision: up for buys, down for sells. Override slippage with
`SubmitOrder.params["market_order_slippage_bps"]`.

### Contingent orders

| Feature                  | Perpetuals | Spot | Notes                                                      |
| ------------------------ | ---------- | ---- | ---------------------------------------------------------- |
| Stop-loss market         | ✓          | -    | `STOP_MARKET` maps to Lighter `STOP_LOSS`.                 |
| Stop-loss limit          | ✓          | -    | `STOP_LIMIT` maps to Lighter `STOP_LOSS_LIMIT`.            |
| Take-profit market       | ✓          | -    | `MARKET_IF_TOUCHED` maps to Lighter `TAKE_PROFIT`.         |
| Take-profit limit        | ✓          | -    | `LIMIT_IF_TOUCHED` maps to `TAKE_PROFIT_LIMIT`.            |
| Trigger price            | ✓          | -    | Required for every supported conditional order.            |
| Trigger source selection | -          | -    | *Not supported by Lighter*; mark-price triggers only.      |
| Grouped order lists      | ✓          | -    | Native OTO, OCO, and OTOCO groups.                         |
| OCO / OTO orders         | ✓          | -    | Same-market groups with SL/TP children.                    |
| Bracket orders           | ✓          | -    | OTO parent with OCO-linked conditional exits.              |
| `CreateGroupedOrders`    | ✓          | -    | One signed transaction and nonce for every leg in a group. |

Submit native groups with `submit_order_list`. The adapter maps Lighter's
[`CreateGroupedOrders`](https://apidocs.lighter.xyz/docs/trading#stop-loss-and-take-profit)
relationships to Nautilus parent and sibling relationships:

| Lighter grouping | Venue code | Nautilus parent | Nautilus children or siblings                                      |
| ---------------- | ---------- | --------------- | ------------------------------------------------------------------ |
| OTO              | 1          | `OTO`           | One child referencing the parent, with no sibling contingency.     |
| OCO              | 2          | None            | Two reciprocally linked `OCO` siblings.                            |
| OTOCO            | 3          | `OTO`           | Two children referencing the parent, reciprocally linked as `OCO`. |

Nautilus has no separate OTOCO type. The adapter maps relationships explicitly because Lighter's
OTO and OCO numeric codes differ from Nautilus codes.

#### Group requirements

- All legs use one perpetual instrument, strategy, and order-list ID, with unique client order IDs.
- An OTO parent uses `MARKET` or `LIMIT`. Its children oppose the parent, use `reduce_only`, and
  request the same quantity as the parent.
- OCO siblings use `reduce_only` and share the same side and quantity. The pair contains one stop-loss
  (`STOP_MARKET` or `STOP_LIMIT`) and one take-profit (`MARKET_IF_TOUCHED` or `LIMIT_IF_TOUCHED`).

Venue expiries depend on the leg:

- Children share a nonzero expiry.
- Market and IOC limit parents use zero expiry.
- Other limit parents share the child expiry.

Default expiries use one timestamp. Before any leg is dispatched, explicit GTD expiries must match
every other nonzero group expiry.

The wire shapes follow the [Go SDK validation](https://github.com/elliottech/lighter-go/blob/9d38261d1a4cc5c7211b383ba07a4d6e41604708/types/txtypes/create_grouped_orders.go).
The adapter additionally requires reduce-only OTO children and matching requested quantities to
preserve Nautilus exit semantics.

For a native bracket, select `OCO` exits and a conditional take-profit. The adapter rejects the
default `OrderFactory.bracket`, which uses `OUO` exits and a plain `LIMIT` take-profit. It does not
replace quantity updates with cancellation or convert resting limits to triggered take-profits.

#### Child activation and sizing

Lighter activates and sizes OTO/OTOCO children from parent execution. Until then, their wire base amount is
zero. Reports for Nautilus-submitted held children retain the requested quantity, and venue quantity changes
produce `OrderUpdated`.

Held children cannot be modified at zero venue quantity. The adapter provides neither full-fill
release control nor independent pro rata sizing.

#### Cancellation behavior

Lighter OCO cancels the sibling when an SL/TP order **triggers**, even if unfilled. Nautilus OCO requests peer
cancellation on a fill or cancellation. The adapter preserves venue trigger-time cancellation without
recreating the sibling. Event ordering can delay cancellation delivery. See
[Native child fills and position reconciliation](#native-child-fills-and-position-reconciliation).

`cancel_order` on a live Nautilus OCO leg requests cancellation of its live peer too, using two
independent transactions: Lighter's single-order cancellation leaves the peer open. Each can succeed or fail.

Await `OrderAccepted` for the target leg. Its peer needs a known venue binding or retained grouped submission
index. `batch_cancel_orders` also includes live OCO peers, submitting each selected order once.

:::warning
Keep `StrategyConfig.manage_contingent_orders = false` (the default) for native groups. Enabling
local contingency management can resize children independently of the venue or cancel a triggered,
unfilled exit when the venue cancels its OCO peer.
:::

#### Trigger events and relationship recovery

Conditional orders use
[mark-price triggers](https://docs.lighter.xyz/trading/order-types-and-matching#trigger-conditions).
Conditional limits emit `OrderTriggered`. Market-style conditionals fill without an intermediate `TRIGGERED`
state, following Nautilus semantics.

With local order context or cached venue bindings, reports preserve Nautilus client, parent, and
linked IDs even after Lighter clears parent links on activation. A fresh client restores cached
ownership before processing its first private-stream order or fill.

Triggered conditional limits retain their Nautilus type and last known trigger price. External
ones without a recoverable trigger price report as working `LIMIT` orders.

After triggering, price and quantity modifications send zero trigger price. Changing it again is unsupported.
Independent transaction batches do not acquire contingency relationships.

### Order options

| Option           | Perpetuals | Spot | Notes                                                                     |
| ---------------- | ---------- | ---- | ------------------------------------------------------------------------- |
| `post_only`      | ✓          | ✓    | Maps to Lighter's post-only time-in-force.                                |
| `reduce_only`    | ✓          | -    | Passed through to `CreateOrder`; use only to reduce an existing position. |
| `quote_quantity` | -          | -    | *Not supported by Lighter*; submit base quantity instead.                 |
| `display_qty`    | -          | -    | *Not supported by Lighter*; no iceberg display quantity field.            |

### Adapter order params

| Param                                      | Perpetuals | Spot | Notes                                               |
| ------------------------------------------ | ---------- | ---- | --------------------------------------------------- |
| `market_order_slippage_bps`                | ✓          | ✓    | Overrides the config default for market-style caps. |
| `post_only` through `SubmitOrder.params`   | -          | -    | *Not currently implemented*; use the order flag.    |
| `reduce_only` through `SubmitOrder.params` | -          | -    | *Not currently implemented*; use the order flag.    |

### Time in force

| Time in force  | Perpetuals | Spot | Notes                                                                         |
| -------------- | ---------- | ---- | ----------------------------------------------------------------------------- |
| `GTC`          | ✓          | ✓    | Limit-style uses `GoodTillTime`; market-style uses `IOC`.                     |
| `DAY`          | ✓          | ✓    | Limit-style and conditional orders use a positive order expiry.               |
| `GTD`          | ✓          | ✓    | Native expiry is 5 minutes to 30 days; see the managed-GTD policy below.      |
| `IOC`          | ✓          | ✓    | Plain `MARKET`/`LIMIT` use expiry `0`; conditional limit uses trigger expiry. |
| `FOK`          | -          | -    | *Not supported by Lighter*.                                                   |
| `AT_THE_OPEN`  | -          | -    | *Not supported by Lighter*.                                                   |
| `AT_THE_CLOSE` | -          | -    | *Not supported by Lighter*.                                                   |

The adapter sends `MARKET`, `STOP_MARKET`, and `MARKET_IF_TOUCHED` as `ImmediateOrCancel`:
Lighter rejects market-style `GoodTillTime`. Expiry and Nautilus `IOC` support differ by order type:

- Plain `MARKET` uses `OrderExpiry = 0`.
- Conditional market orders retain positive expiry until triggering. The adapter rejects Nautilus
  `IOC` for them because Lighter reserves IOC for post-trigger execution.
- Conditional limit orders accept Nautilus `IOC`: the trigger rests with positive expiry, then
  the child executes as `ImmediateOrCancel`.

Without explicit GTD expiry, limit-style `GTC`, `DAY`, and `GTD` use now plus 28 days. Conditional
`GTC`, `DAY`, and limit-style `IOC` share this default: the venue rejected `-1` in these paths with
`21711 invalid expiry`.

#### GTD policy

With the default `use_gtd = true`, the strategy's `expire_time` becomes the venue `GoodTillTime` expiry. The
adapter accepts lifetimes in `[5 minutes + 1 second, 30 days]`. The extra second allows for signing and
transport.

For shorter lifetimes, set `use_gtd = false` only with `manage_gtd_expiry = true` on the submitting strategy.
The local GTD manager requests cancellation at strategy expiry. Without it, the order can rest until the
28-day venue fallback expiry.

The wire time-in-force remains `GoodTillTime`: Lighter has no `GoodTillCancel`, so `use_gtd = false`
cannot switch it as on Binance.

Local management skips the native 5-minute minimum but rejects strategy expiries beyond 28 days,
when the venue would expire first. Use native GTD for longer lifetimes. Venue cancel latency still
delays removal.

### Execution instructions

| Instruction   | Perpetuals | Spot | Notes                                                     |
| ------------- | ---------- | ---- | --------------------------------------------------------- |
| `post_only`   | ✓          | ✓    | Overrides the TIF and sends Lighter `PostOnly`.           |
| `reduce_only` | ✓          | -    | Position-reducing flag for existing derivative positions. |

Use `post_only` on limit-style orders. The adapter does not synthesize maker-only market orders.

Lighter Mainnet testing confirms `reduce_only=true` closes perpetual positions. Invalid reduce-only
opens can be dropped without a venue order report and reconcile as `INFLIGHT_TIMEOUT`, without
a venue rejection reason.

### Advanced order features

| Feature            | Perpetuals | Spot | Notes                                                      |
| ------------------ | ---------- | ---- | ---------------------------------------------------------- |
| Order modification | ✓          | ✓    | Modify quantity, price, and trigger price on a live order. |
| Bracket orders     | ✓          | -    | Native OTO parent with OCO-linked SL/TP exits.             |
| Iceberg orders     | -          | -    | *Not supported by Lighter*.                                |
| Trailing stops     | -          | -    | *Not supported by Lighter*.                                |
| Pegged orders      | -          | -    | *Not supported by Lighter*; no native pegged API type.     |
| TWAP orders        | -          | -    | *Not currently implemented*; no Nautilus mapping.          |
| Chase limit orders | -          | -    | *Not currently implemented*.                               |
| Atomic baskets     | -          | -    | *Not currently implemented*.                               |
| Leverage update    | ✓          | -    | Perp only; submits a signed `UpdateLeverage` tx.           |
| Native cancel-all  | -          | -    | *Not currently implemented* in the execution client.       |
| Dead man's switch  | -          | -    | *Not currently implemented*; venue has scheduled cancel.   |

Lighter's [`CancelAllOrders`](https://apidocs.lighter.xyz/docs/wasm-signer#signcancelallorders) supports
immediate and scheduled cancellation. The execution client uses per-order cancellations. `lighter-flatten`
uses immediate native account-wide cancellation. The adapter does not schedule or renew cancellation
deadlines.

Venue
[chase limit orders and atomic baskets](https://docs.lighter.xyz/trading/order-types-and-matching#advanced-orders)
are not exposed. Ordinary transaction batches execute each child independently.

### Order operations

| Operation           | Perpetuals | Spot | Notes                                                        |
| ------------------- | ---------- | ---- | ------------------------------------------------------------ |
| Submit order        | ✓          | ✓    | Sends a signed `L2CreateOrder` transaction over WebSocket.   |
| Submit order list   | ✓          | ✓    | Independent batches; native contingent groups on perps only. |
| Modify order        | ✓          | ✓    | Sends a signed `ModifyOrder`; reports may restate accepts.   |
| Batch modify orders | ✓          | ✓    | WebSocket batches of up to 15 signed modify transactions.    |
| Cancel order        | ✓          | ✓    | Sends a signed `L2CancelOrder` transaction.                  |
| Cancel all orders   | ✓          | ✓    | Cancels cached orders by instrument and optional side.       |
| Set leverage        | ✓          | -    | Perp only; submits a signed `UpdateLeverage` tx.             |
| Batch cancel orders | ✓          | ✓    | WebSocket batches of up to 15 signed cancel transactions.    |
| Query order         | ✓          | ✓    | Requires credentials and REST lookup.                        |
| Query account       | ✓          | ✓    | Replays the latest private WebSocket account state.          |
| Mass status         | ✓          | ✓    | Bounded to account-active markets from WS and REST reports.  |

#### Order lists and batch operations

Non-contingent `SubmitOrderList`, `BatchModifyOrders`, and `BatchCancelOrders` use WebSocket
`jsonapi/sendtxbatch`, retaining each child's signed transaction and order-event attribution.
They provide neither atomic execution nor contingency relationships.

Local validation denies invalid submit children or rejects invalid modify children while supported
siblings continue. Batches hold up to 15 transactions: 31 supported children normally produce
15, 15, and 1. Concurrent nonce allocation or recovery can produce smaller batches.

Each batch uses a separate rate-limiter token and acknowledgement. Rejection affects only its children.

Before signing the next chunk, the adapter waits up to 10 seconds for the current chunk's
acknowledgements. On timeout, dispatch continues and retains unacknowledged entries for reconciliation.

`CancelAllOrders` uses the same batching path, preserving any side filter.
Venue [active-order limits](#active-and-pending-order-limits) still apply: splitting a command does not
increase the number of orders the account can hold on a market.

A contingent list must contain exactly one supported native group of two or three perpetual
orders, sharing one `CreateGroupedOrders` transaction and nonce. Any invalid leg denies the whole group.

Lighter's [trading guide](https://apidocs.lighter.xyz/docs/trading#batch) gives a maximum of 50, while its
[WebSocket reference](https://apidocs.lighter.xyz/docs/websocket-reference#send-batch-tx) states 15.
The adapter uses the lower documented limit.

#### Nonce allocation and transaction admission

Each batch uses one account and API key with consecutive nonces under Lighter's
[nonce contract](https://apidocs.lighter.xyz/docs/core-concepts#nonce). With `skip_nonce=0`, the
adapter must preserve nonce order.

The local window allows 16 unconfirmed allocations per key, not out-of-order venue transactions. Unsigned
children wait for capacity or nonce recovery without being discarded. Shutdown stops dispatch and drops
remaining queued work.

Batch responses correlate by request ID and signed transaction hash.

An acknowledgement confirms transaction admission, not order acceptance, modification, or
cancellation. Failures depend on when rejection occurs:

- Pre-admission rejection fails the whole batch without consuming nonces. An invalid-nonce
  response also refreshes the nonce baseline.
- After admission, individual transactions can fail while others succeed. Application failures consume the
  nonce. Sequencer rejection before application can leave it unconsumed and invalidate later nonces on that
  key.

See [nonce consumption](https://apidocs.lighter.xyz/docs/core-concepts#when-a-rejected-transaction-consumes-its-nonce).

On ambiguous delivery, the adapter retains pending state for reconciliation and does not resend the batch.

#### Modification confirmation

Only one modification per order can remain unresolved. Wait for its `OrderUpdated` or `OrderModifyRejected`
before modifying again. A second request receives `OrderModifyRejected` without resolving the first.

Pending cancellation rejects modification. During a pending modification, cancellation remains
available but its outcome waits for modification resolution.

Confirmation requires a transaction lookup and matching current venue order shape. Affected
WebSocket order frames and fills wait until the adapter publishes `OrderUpdated` or
`OrderModifyRejected`. HTTP reports that could apply the amendment or fill wait for the cache to
apply that event.

| Request                            | Behavior while modification is pending                     |
| ---------------------------------- | ---------------------------------------------------------- |
| Single-order status lookup         | Can return an unchanged working report as `PendingUpdate`. |
| Bulk order reports and mass status | Withhold the order and mark the result incomplete.         |
| `query_order`                      | Emits no report.                                           |

This can add several seconds of reporting delay. Position reconciliation withholds affected
instruments until their execution events reach the cache.

#### Native child fills and position reconciliation

For native OTO/OTOCO children, real fills wait until a terminal venue order and complete real trade
coverage establish effective quantity. The adapter publishes that quantity before fills and
reconciles terminal status afterward.

Resting children publish trigger events once their modification resolves.

An unresolved child fill or modification also holds later fills and terminal events for the same strategy and
instrument. Fills release in venue event-time order, with arrival order breaking ties. HTTP reports that could
apply those changes wait for the cache to catch up.

Cached fills and position accounting can lag until the child closes and real trade coverage is complete.

During this delay:

- Position queries fail for a scope containing an affected instrument.
- Mass status omits affected position reports and synthetic flat reports and remains incomplete.
- Venue position snapshots remain available on the private stream.

#### Outcome recovery

Order updates and transaction lookups resolve create and cancel children. Recovery makes up to
three lookups at two-second intervals while the pending action remains current, validating
transaction hash, type, account, API key, and original submission nonce.

A definitively failed admitted modification, single or batched, emits one `OrderModifyRejected`
and clears its matching pending action without rolling back the consumed nonce.

Delayed outcomes cannot clear a newer modification or reject one already confirmed by recovery.

:::warning
If bounded lookups cannot confirm a modification, it can remain unresolved for the rest of the session. This
holds later order events and fills, and rejects further modifications. The adapter neither assumes success nor
resends it. Restart and reconcile venue state before managing that order again.
:::

Modification probes retain their original submission identity across reconnects. Cancellation probes stop when
their connection changes.

Cancellation recovery includes orders outside `PendingCancel`, such as `CancelAllOrders` targets. A cached
order receives `OrderCancelRejected` only if it was `PendingCancel` at dispatch. Otherwise, failure is logged.

#### Leverage updates and signing validation

Call `LighterExecutionClient::update_leverage(instrument_id, initial_margin_fraction, margin_mode)`.
The margin fraction uses venue ticks (1e-4 fraction): `500` is 5% initial margin (20x leverage),
and `1000` is 10% (10x).

`UpdateLeverage`, `CancelAllOrders`, modify orders with integrator attributes, conditional create
orders, and `CreateGroupedOrders` are byte-pinned against the signer distributed with the official
`lighter-python` SDK version 1.1.4.

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

Authenticated inactive-order and fill pagination rejects repeated cursors and stops after
1,000 pages. Fill reconciliation is repeatable across calls, suppressing fills already emitted by
the live WebSocket stream.

Historical reports bind mapped client indexes only to matching venue order IDs, preventing reused
indexes from merging unrelated lifecycles.

#### Report completeness

Each bounded mass status uses one cutoff for inactive orders and fills. Completeness requires all
required order, fill, and position sources to succeed and every historical fill to map to its order.

Held events, unresolved modifications, or updates not yet applied to the cache make withheld order,
fill, and position sources incomplete.

History failure alone does not veto available active orders or explicit position reports.
Bounded historical fills without an in-scope position report follow the engine's
[order-only projection](../concepts/execution/reconciliation.md#order-only-fill-projection) rules.

The [`trades` reference](https://apidocs.lighter.xyz/reference/trades) does not guarantee history depth, so a
lookback can exceed available fills. Pagination walks back from the newest trade. Only a trade older than the
lookback start proves coverage:

- Trade older than the start: the report set stays complete.
- Cursor exhausted first: the adapter logs the uncovered span and marks the report set incomplete.
- No retained trades: nothing can have been truncated, so the report set stays complete.

Cursor exhaustion cannot distinguish truncation from having no older trades: young accounts can
report incomplete with nothing missing. Choose a lookback the venue can serve.

For older fills, the venue's [historical data exports](https://apidocs.lighter.xyz/docs/historical-data)
provide up to 12 months of account trades. The adapter does not read the `export` endpoint.

#### Startup position checks

Opening a position at strategy startup can produce a transient `cached=0, venue=N` warning if
`account_all_positions` arrives milliseconds before its matching fill is processed. Applying the
fill resolves the discrepancy without generating reconciliation orders.

## Account and position management

Authenticated execution clients subscribe to these private streams:

- `account_all_orders`: order status reports.
- `account_all_trades`: fill reports.
- `account_all_positions`: initial position snapshot and live updates.
- `account_all_assets`: per-asset balance snapshots (spot balance plus perp collateral).
- `user_stats`: perp-account margin rollup (collateral and available balance).

The adapter emits merged `account_all_assets` and `user_stats` state only after both first frames arrive.

Construction allows missing credentials. `connect()` requires resolved `private_key`, `account_index`, and
`api_key_index`. Private account streams and nonce refresh are mandatory.

### Position snapshots and updates

Perpetuals use one net position per market. Spot balances use account asset state. The authoritative
`subscribed/account_all_positions` snapshot:

- Flattens omitted markets and zero-`position` rows. An empty `positions` map flattens the entire cache.
- Retains cached positions for unmapped or unparsable rows to prevent false flat reports.

Bounded reconciliation tracks snapshot market coverage, which reconnect invalidates. An absent
touched market produces an explicit flat report only with current snapshot coverage. Unmapped or
malformed rows leave mass status incomplete instead of proving flat.

Incremental `update/account_all_positions` frames replace nonzero positions and flatten explicit
zero rows, retaining omitted markets. Empty updates retain all positions.

| Feature                 | Perpetuals | Spot | Notes                                                       |
| ----------------------- | ---------- | ---- | ----------------------------------------------------------- |
| Account balances        | ✓          | ✓    | Merged assets + `user_stats`, replayed from cache on query. |
| Position state          | ✓          | -    | Perp only; initial snapshot plus live updates.              |
| Netting positions       | ✓          | -    | One Nautilus position per perpetual market.                 |
| Cross margin            | ✓          | -    | Passed through `LighterPositionMarginMode::Cross`.          |
| Isolated margin         | ✓          | -    | Passed through `LighterPositionMarginMode::Isolated`.       |
| Leverage updates        | ✓          | -    | Signed `UpdateLeverage` transaction.                        |
| Spot margin / borrowing | -          | -    | *Not currently implemented*; no borrowing workflow.         |
| Collateral-mode changes | -          | -    | *Not currently implemented*; configure with venue tools.    |
| Deposits / withdrawals  | -          | -    | *Not currently implemented*; use venue tools or APIs.       |

Lighter's [multi-asset margin](https://docs.lighter.xyz/trading/multi-asset-margin) supports non-USDC
perpetual collateral. Its documentation describes spot use cases but excludes spot at launch. The adapter
exposes neither spot borrowing nor collateral-mode changes. The spot-margin row describes an adapter gap, not
a venue limitation.

## Liquidation and ADL handling

| Event or field              | Support | Notes                                                         |
| --------------------------- | ------- | ------------------------------------------------------------- |
| Liquidation trades          | ✓       | Account trade rows can parse as fills, with no special event. |
| Deleverage trades           | ✓       | Account trade rows can parse as fills, with no special event. |
| Liquidation price reporting | -       | *Not currently implemented*; position reports omit the field. |
| ADL event stream            | -       | *Not currently implemented*; no notification subscription.    |

Lighter position payloads include `liquidation_price`. Its authenticated
[`notification` stream](https://apidocs.lighter.xyz/docs/websocket-reference#notification) publishes
liquidation and deleverage notifications.

The adapter parses liquidation prices in raw payloads without exposing them in Nautilus position
reports or subscribing to notifications. Liquidation and deleverage trades use the normal fill-report path.

## Funding rates

Perpetual `market_stats` emits `MarkPriceUpdate`, `IndexPriceUpdate`, and `FundingRateUpdate`.
Funding fields differ:

- `current_funding_rate`: upcoming estimate used in live updates.
- `funding_rate` and `funding_timestamp`: last completed payment.

Market stats provide no future settlement time, so live updates leave `interval` and `next_funding_ns`
unset. Spot `spot_market_stats` emits `IndexPriceUpdate`.

Historical funding requests use public `/api/v1/fundings`, not account-specific `positionFunding`:

- `1h` resolution sets `interval=60`.
- `direction=long` stays positive. `short` becomes negative.
- Pagination covers the requested range up to the adapter's [page cap](#historical-request-limits),
  subject to an explicit `limit`.

## Account tiers

Account tiers set latency, rate limits, and trading fees. The client logs the tier from
`GET /api/v1/account`, including unknown `account_type` values.

All Lighter Mainnet tiers use [zero-fee integrator attribution](#integrator-attribution).
Quotas never rise automatically, and local overrides do not grant higher venue limits.

For [Lighter Mainnet](https://apidocs.lighter.xyz/docs/account-types):

| Tier     | Latency (maker / taker) | REST weighted limit | `sendTx` limit          | Fees (maker / taker)            | Notes                                   |
| -------- | ----------------------- | ------------------- | ----------------------- | ------------------------------- | --------------------------------------- |
| Standard | 0 ms / 300 ms           | 60 req/min          | 60 req/min              | 0 / 0                           | Zero-fee default tier.                  |
| Premium  | 0 ms / 140 ms           | 24,000 req/min      | [4,000, 57,600] req/min | [0.28, 0.40] / [1.96, 2.80] bps | Lowest latency; scales with staked LIT. |
| Plus     | 0 ms / 300 ms           | 24,000 req/min      | 4,000 req/min           | 0.5 / 0.5 bps                   | Raised limits, standard latency.        |
| Builder  | -                       | 240,000 req/min     | -                       | -                               | Highest REST throughput.                |

Premium limits and fees scale with staked LIT. Robinhood uses different fees and limits, with Premium tiers
based on 14-day trading volume. See
[Robinhood account tiers](https://apidocs.lighter.xyz/docs/lighter-rh#account-tiers). Before raising local
quotas, confirm deployment limits for your traffic. See [Rate limiting](#rate-limiting).

## Rate limiting

Lighter limits both IP and L1 addresses. Data and execution clients have separate REST limiters
defaulting to Standard quotas. Budget their combined traffic within venue limits.

Higher [tiers](#account-tiers) require explicit local quotas:

- `rest_quota_per_min`: read requests per minute, on both clients. Unset defaults to 60.
- `sendtx_quota_per_min`: transaction requests per minute, on execution clients only. Unset defaults to 60
  independently of `rest_quota_per_min`, in a separate bucket.

These options change only local pacing. Public data remains unauthenticated. Higher local quotas do not
qualify it for account-level venue limits.

### Request buckets and endpoint weights

The venue shares one transaction bucket per L1 address across HTTP and WebSocket. A WebSocket
`sendTxBatch` counts once, carrying at most 15 adapter transactions.

Standard accounts share 60 requests/minute across reads and transactions. Plus and Premium use separate venue
buckets. The adapter separates local read and transaction limiters on every tier, so Standard users must
budget combined traffic, including nonce and reconciliation reads.

The REST limiter spends one token per call, ignoring venue weights. Set `rest_quota_per_min` for your endpoint
mix. Lighter Mainnet's 24,000 weighted req/min Premium limit permits either of these rates before other
requests:

- 40 `/api/v1/recentTrades` calls/minute (weight 600).
- 120 `/api/v1/trades` calls/minute (weight 200).

Confirm weights for your deployment.

### Transaction type limits

Lighter's default transaction-type limit is 40 requests/minute, with type-specific exceptions,
separate from tier `sendTx`/`sendTxBatch` limits.

Lighter Mainnet quoting that amended on every quote drift hit `code=23000` (`Too Many Requests`) after
roughly 40 modifications in a minute. Modifications also spend
[volume quota](#volume-quota-and-no-fill-quoting).

For transaction-heavy quoting, set `sendtx_quota_per_min` to 40 or lower. This shared limiter also
paces creates and cancels. Batches spend one token for up to 15 transactions, so the setting does
not cap child counts in order lists or batch modifications. Confirm deployment transaction-type
limits for batched children.

Local quotas allow bursts, not strict rolling-minute ceilings. A quota of 30 permits an initial
30-request burst and replenishes at 30/minute, potentially exceeding a venue limit of 40 in 60 seconds.
Leave headroom for bursts and other clients sharing the L1 address.

For bounded Standard testing, 15 transaction requests and 5 REST calls/minute leave room for
independent account checks.

An uncorrelated WebSocket `23000` response is logged without rejecting a particular transaction. It can apply
to non-transaction traffic, so the adapter does not guess which order or batch failed. Pending outcomes
require reconciliation. Do not resend them blindly.

### Shared adapter limiters

One execution limiter enforces `sendtx_quota_per_min` across WebSocket `sendTx` and `sendTxBatch` (submit,
modify, and cancel) and HTTP `sendTx` for startup integrator approval. Raw calls use it when configured.
Otherwise, they fall back to the raw client's REST limiter.

Clients with identical WebSocket URL strings share a 200-message/minute limiter for non-transaction control
frames. The default data URL's `readonly=true` gives it a separate limiter from execution. Budget combined
control traffic within the venue per-IP limit.

Each connection's subscription gate permits 35 unacknowledged requests, a count governed by
acknowledgement latency rather than send rate. It does not enforce the venue's 50-message per-IP
ceiling across connections. `sendTx` and `sendTxBatch` consume neither the client-message bucket
nor the venue inflight cap.

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

Lighter's [tier limits](https://apidocs.lighter.xyz/docs/rate-limits) apply per account and per market
within that account:

| Account tier | Active per account | Active per market | Pending per account | Pending per market |
| ------------ | ------------------ | ----------------- | ------------------- | ------------------ |
| Standard     | 250                | 30                | 50                  | 10                 |
| Plus         | 750                | 250               | 500                 | 100                |
| Premium      | 1,500              | 1,000             | 1,000               | 100                |

Active orders rest on the book. Venue-pending orders include untriggered take-profit, stop-loss, and TWAP,
distinct from Nautilus `PendingCancel` or unacknowledged requests. The adapter does not pre-count them. Leave
room for existing orders. Local quotas do not raise venue caps.

### Endpoint weights and transport limits

Common REST weights from the [Lighter Mainnet rate limits](https://apidocs.lighter.xyz/docs/rate-limits):

| Endpoint group                                                  | Weight | Adapter behavior                                |
| --------------------------------------------------------------- | ------ | ----------------------------------------------- |
| `sendTx`, `sendTxBatch`, `nextNonce`                            | 6      | Tx calls use tx limiter; `nextNonce` uses REST. |
| `accountInactiveOrders`, `accountActiveOrders`, `accountOrders` | 100    | Adapter counts one REST token per HTTP call.    |
| `apikeys`                                                       | 150    | Adapter counts one REST token per HTTP call.    |
| `trades`                                                        | 200    | Adapter counts one REST token per HTTP call.    |
| `recentTrades`                                                  | 600    | Adapter counts one REST token per HTTP call.    |
| Endpoints without an explicit venue weight                      | 300    | Adapter counts one REST token per HTTP call.    |

Other named endpoints have distinct weights. Check the official table before budgeting calls. Robinhood
weights can differ. See [Robinhood rate limits](https://apidocs.lighter.xyz/docs/lighter-rh#rate-limits).

| Endpoint or transport                  | Limit               | Notes                                                            |
| -------------------------------------- | ------------------- | ---------------------------------------------------------------- |
| `/api/v1/trades`                       | 100 rows            | Adapter paginates reconciliation at this cap.                    |
| `/api/v1/accountInactiveOrders`        | 100 rows            | Adapter follows `next_cursor` at this cap.                       |
| `/api/v1/orderBookOrders`              | 250 orders per side | Adapter aggregates these orders into L2 price levels.            |
| `/api/v1/candles`                      | 500 rows            | Adapter caps REST bar pages at this venue maximum.               |
| `/api/v1/fundings`                     | 750 rows            | Venue cap; adapter requests 100 rows per page.                   |
| WebSocket connections                  | 255 / IP            | Venue limit.                                                     |
| WebSocket subscriptions / connection   | 500                 | Venue limit.                                                     |
| WebSocket subscriptions / IP           | 5,000               | Venue limit.                                                     |
| WebSocket unique accounts / connection | 500                 | Venue limit.                                                     |
| WebSocket unique accounts / IP         | 5,000               | Venue limit.                                                     |
| WebSocket connections / minute         | 255                 | Venue limit.                                                     |
| WebSocket client messages / minute     | 200                 | Paces non-tx frames; heartbeat pings bypass it.                  |
| WebSocket inflight messages            | 50 / IP             | Subscription gate allows 35 outstanding requests per connection. |
| WebSocket `sendTxBatch` batch size     | 15 txs              | Adapter cap per request; larger commands split into batches.     |
| WebSocket keepalive                    | 2 minutes           | Adapter sends heartbeats every 30 seconds.                       |
| WebSocket outbound command queue       | Not capped          | Paced before writes; no queue-depth cap.                         |

### Historical request limits

Bar and funding history cap at 500 REST pages: up to 250,000 bars or 49,500 hourly funding intervals.
Completion on the final page succeeds, as does an explicit-`start` request that satisfies its explicit
`limit`.

Uncovered ranges return `LighterHttpError::HistoryIncomplete` without partial history or retry. The data
client logs the error and emits no response. Narrow the range to continue.

## Volume quota and no-fill quoting

Separate from transport limits, [volume quota](https://apidocs.lighter.xyz/docs/volume-quota-program)
applies only to Plus and Premium accounts, shared across sub-accounts under one L1 address:

- `L2CreateOrder`, `L2CancelAllOrders`, `L2ModifyOrder`, and `L2CreateGroupedOrders` spend it. Each eligible
  batched transaction spends separately. Single-order cancellation does not.
- Completed trading volume and the free allowance replenish it.

The adapter does not inspect remaining quota. See the linked venue documentation for current rules
and figures.

Repeated no-fill quote refreshes can exhaust this quota even when the WebSocket and `sendTx`
limiters work. For live tests, prefer slower one-sided quoting, wider refresh thresholds, testnet,
or a bounded strategy that earns enough fills to replenish its quota.

## Connection management

### Heartbeats and reconnects

The WebSocket client sends heartbeats every 30 seconds and reconnects with exponential backoff in
[250 milliseconds, 30 seconds]. After 90 seconds without an inbound frame, it reconnects even if
the venue leaves the stalled socket open. Pongs refresh this window without market data.

### Private authentication

Private auth tokens have an 8-hour maximum lifetime. The adapter mints 7-hour tokens and rotates and
resubscribes every 6 hours. Transparent reconnect mints a fresh token and resubscribes account
streams after tracked subscriptions start replaying.

### Nonce recovery

Execution reconnect refreshes the nonce baseline through `GET /api/v1/nextNonce`. Single submit,
modify, and cancel commands cannot sign until refresh or its background retry installs the new
connection's baseline. Batch commands, including those received during refresh, wait for readiness.

Within a session:

- Venue confirmations advance the local nonce window.
- Definitive rejections or pre-handoff failures may roll back its latest nonce.
- Stale state triggers a `GET /api/v1/nextNonce` resync.

Outcomes that may have reached the venue retain their pending nonce and order identity for
WebSocket or reconciliation recovery.

### Account stream readiness

`LighterExecutionClient::connect()` clears old position and account caches, then waits up to 30
seconds for all five [private streams](#account-and-position-management) to become ready.

Only `subscribed/account_all_positions` satisfies position readiness. Live updates do not. REST account
payloads are never a fallback.

Transparent reconnects and auth rotations do not re-enter `connect()`. They retain positions until
the next `subscribed/account_all_positions` applies snapshot replacement rules. Live updates merge
without evicting omitted markets.

## API credentials

Signing requires three credentials:

- Account index: numeric Lighter account identifier.
- API key index: numeric API key slot. Use an unreserved index in `[4, 254]`. Robinhood also reserves `157`.
  Do not use `[0, 3]` or `255`. `255` is an `apikeys` query sentinel, not a signing key.
- API private key: 40-byte hex private key, with or without a `0x` prefix.

Config values take precedence. Missing fields or an empty or whitespace-only private key fall
back to the environment variable selected by `deployment` and `environment`.

| Deployment | Environment | API key index                             | API private key                        | Account index                             |
| ---------- | ----------- | ----------------------------------------- | -------------------------------------- | ----------------------------------------- |
| Lighter    | Mainnet     | `LIGHTER_API_KEY_INDEX`                   | `LIGHTER_API_SECRET`                   | `LIGHTER_ACCOUNT_INDEX`                   |
| Lighter    | Testnet     | `LIGHTER_TESTNET_API_KEY_INDEX`           | `LIGHTER_TESTNET_API_SECRET`           | `LIGHTER_TESTNET_ACCOUNT_INDEX`           |
| Robinhood  | Mainnet     | `LIGHTER_ROBINHOOD_API_KEY_INDEX`         | `LIGHTER_ROBINHOOD_API_SECRET`         | `LIGHTER_ROBINHOOD_ACCOUNT_INDEX`         |
| Robinhood  | Testnet     | `LIGHTER_ROBINHOOD_TESTNET_API_KEY_INDEX` | `LIGHTER_ROBINHOOD_TESTNET_API_SECRET` | `LIGHTER_ROBINHOOD_TESTNET_ACCOUNT_INDEX` |

The four namespaces let one process run clients for multiple deployment targets without sharing
credentials.

Execution rejects incomplete credentials. All data subscriptions and REST requests for instruments,
books, trades, bars, and funding use public endpoints without credentials.

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
| `use_gtd`                   | `true`        | Use venue-native GTD; see [GTD policy](#gtd-policy).          |

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

These execution configs use the [credential environment variables](#api-credentials) for their deployment and
environment. Explicit credential fields override them.

Set `LiveExecutionEngineConfig.reconciliation_instrument_ids` to scope reconciliation and
`reconciliation_lookback_mins` to bound inactive-order and fill replay.

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
To contribute to the Lighter adapter or request a feature, see the
[contributing guide](https://github.com/nautechsystems/nautilus_trader/blob/develop/CONTRIBUTING.md).
:::
