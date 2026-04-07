# Complement Arbitrage on Polymarket Binary Options

This tutorial walks through running a complement arbitrage strategy on Polymarket using the
Rust-native `LiveNode`.

**What you'll learn:**

- How complement arbitrage works on binary options and why `Yes + No = 1.0` creates
  risk-free profit opportunities.
- How to configure the Polymarket data pipeline — startup filters, WebSocket market
  discovery, and instrument pairing by condition ID.
- How the per-pair state machine manages execution risk: entry orders, partial fill
  detection, and automatic unwind.
- How to run the strategy in detection-only mode, tune parameters, and graduate to
  live execution.

## Introduction

### What is complement arbitrage?

Binary options have a mathematical constraint: at resolution, `Yes + No = 1.0`. A Yes
contract pays $1 if the outcome is true and $0 otherwise; a No contract pays the opposite.
Holding both sides guarantees a $1 payout regardless of the outcome.

When market inefficiencies cause the combined ask price of a Yes/No pair to fall below $1.00,
buying both sides locks in risk-free profit. When the combined bid exceeds $1.00, selling both
sides does the same. These are **buy arbs** and **sell arbs** respectively.

```
Buy arb:  yes_ask + no_ask < 1.0   →  cost < guaranteed payout
Sell arb: yes_bid + no_bid > 1.0   →  revenue > guaranteed liability
```

### Why Polymarket?

Polymarket is the largest prediction market by volume, with thousands of active binary option
markets across politics, sports, crypto, and current events. Several features make it
well-suited for complement arbitrage:

- **Binary option pairs**: every market has a Yes and No token sharing a condition ID.
  These are mathematically complementary — one must resolve to $1 and the other to $0.
- **On-chain settlement**: CTF (Conditional Token Framework) tokens settle deterministically
  on Polygon. Holding Yes + No guarantees a $1 redemption.
- **Deep liquidity on popular markets**: sports and political markets routinely have
  $100k+ in liquidity, providing enough depth for meaningful arb sizes.
- **WebSocket market discovery**: the adapter supports real-time new market notifications,
  enabling the strategy to discover and monitor new pairs as they are listed.
- **Post-only orders**: maker orders have 0% fee on Polymarket, eliminating fee drag on
  arb execution.

### Complement pair mechanics

Each Polymarket market creates two CLOB tokens sharing a condition ID. The adapter represents
these as `BinaryOption` instruments with IDs like:

```
0xabc...def-<yes_token_id>.POLYMARKET   (outcome: "Yes")
0xabc...def-<no_token_id>.POLYMARKET    (outcome: "No")
```

The strategy groups instruments by the hex prefix before the last `-` (the condition ID) and
matches pairs where one has outcome `"Yes"` and the other `"No"`.

## Prerequisites

### Polymarket account

You need a Polymarket account with:

1. **A wallet for signing orders.** Polymarket supports three signing modes
   (configured via `signature_type` on the execution client):
   - **`PolyGnosisSafe` (recommended)** — proxy wallet backed by a Gnosis Safe.
     This is what you get when you create an account through the Polymarket web UI
     by connecting MetaMask (or any browser wallet). Two big advantages: order
     submission is **gas-free** (the Polymarket relayer pays gas) and the same
     account is visible in the Polymarket web GUI, so you can monitor positions
     and intervene manually if needed.
   - **`PolyProxy`** — older proxy wallet variant. Use this if you have a legacy
     Polymarket account that was created before the Gnosis Safe migration.
   - **`Eoa`** — direct on-chain signing with an EOA private key. No proxy, no
     relayer, no GUI integration — you pay gas yourself. Intended for advanced
     users who want direct on-chain control. Most traders should not start here.
2. **API credentials** (key, secret, passphrase) for the L2 CLOB API.
3. **USDC.e allowance** set for the CTF Exchange contract on Polygon.

For the proxy wallet modes (`PolyGnosisSafe` / `PolyProxy`), `POLYMARKET_PK` is
the private key of the **owner** wallet (e.g. your MetaMask key) that controls
the proxy — *not* a separate trading key — and `POLYMARKET_FUNDER` is the
on-chain address of the proxy itself (visible in the Polymarket UI under your
profile / settings).

See the [Polymarket integration guide](../integrations/polymarket.md) for detailed setup
instructions, including wallet configuration, allowance scripts, and API key generation.

### Environment variables

```bash
# Required: signer private key (hex, with or without 0x prefix).
# In proxy modes (PolyGnosisSafe / PolyProxy) this is the *owner* wallet —
# e.g. the MetaMask private key that controls your Polymarket proxy.
# In Eoa mode it is the trading key itself.
export POLYMARKET_PK="0x..."

# Required: L2 API credentials.
export POLYMARKET_API_KEY="your-api-key"
export POLYMARKET_API_SECRET="your-api-secret"
export POLYMARKET_PASSPHRASE="your-passphrase"

# Required for proxy modes: the on-chain address of your Polymarket proxy
# (Gnosis Safe or PolyProxy contract). Find it in the Polymarket UI under
# your profile / settings.
export POLYMARKET_FUNDER="0x..."
```

Alternatively, place these in a `.env` file in the project root (loaded automatically
via `dotenvy`).

#### Generating API credentials

If you don't already have CLOB API credentials, the adapter ships with a small
helper binary that signs an EIP-712 `ClobAuth` message with your `POLYMARKET_PK`
and calls the CLOB auth endpoints to create or derive a key/secret/passphrase
triple for that signer:

```bash
export POLYMARKET_PK="0x..."  # owner wallet for proxy modes, trading key for Eoa
cargo run -p nautilus-polymarket --bin polymarket-create-api-key
```

The script prints the credentials to stdout. Copy them into your `.env` (or
export them as `POLYMARKET_API_KEY` / `POLYMARKET_API_SECRET` /
`POLYMARKET_PASSPHRASE`) and you're set. Source: `crates/adapters/polymarket/bin/create_api_key.rs`.

## Strategy overview

### Pair discovery

On startup, the strategy:

1. Queries the instrument cache for all `BinaryOption` instruments on the configured venue.
2. Groups instruments by condition ID (extracted from the symbol via `rfind('-')`).
3. Matches groups of exactly two instruments where one has outcome `"Yes"` and the other
   `"No"`.
4. Subscribes to quotes for both legs of each discovered pair.
5. Subscribes to new instruments on the venue for dynamic pair discovery.

When new instruments arrive via the `on_instrument` handler, the strategy performs an O(1)
lookup against pending unpaired instruments. If the complement is already pending, a pair
is formed immediately and quotes are subscribed. Otherwise the instrument is stored as
pending until its complement arrives.

### Buy arb detection

A buy arb exists when the cost of buying both sides is less than the guaranteed $1 payout:

```
combined_ask = yes_ask + no_ask
fee          = leg_fee(yes_ask) + leg_fee(no_ask)
               where leg_fee(p) = fee_estimate_bps / 10_000 × p × (1 - p)
profit_bps   = (1.0 - combined_ask - fee) * 10_000
```

The strategy triggers when:

- `combined_ask < 1.0` (raw arb exists)
- `profit_bps >= min_profit_bps` (profit exceeds minimum threshold)
- `ask_size >= trade_size` on both legs (sufficient liquidity)

### Sell arb detection

A sell arb exists when selling both sides collects more than the $1 liability:

```
combined_bid = yes_bid + no_bid
fee          = leg_fee(yes_bid) + leg_fee(no_bid)
               where leg_fee(p) = fee_estimate_bps / 10_000 × p × (1 - p)
profit_bps   = (combined_bid - fee - 1.0) * 10_000
```

Same trigger conditions apply, using bid prices and bid sizes.

### Execution

#### Why execution is the hard part

Detecting a complement arb is straightforward — if `yes_ask + no_ask < 1.0` after fees,
there's profit. The challenge is *capturing* that profit. Polymarket's CLOB does not
support atomic cross-instrument orders, so the strategy must submit two independent limit
orders (one per leg) and manage the risk that only one fills.

Without proper execution logic, three things can go wrong:

1. **Stale arbs**: the spread closes before orders fill, resulting in no execution or
   an unfavorable fill.
2. **Partial fills**: one leg fills but the other is rejected, expires, or gets cancelled.
   You now hold a directional binary option position — one side resolves to $0.
3. **Duplicate entries**: a new arb signal fires on the same pair while the previous
   attempt is still pending, doubling exposure.

The strategy addresses these with a per-pair state machine, GTD expiry, and automatic
unwind logic.

#### Per-pair state machine

Each complement pair has an independent execution state:

```
Idle ──[arb detected]──→ PendingEntry ──[both legs fill]──→ Idle (arb complete)
                              │
                    [one fills, other fails]
                              │
                              ▼
                        PartialFill ──[other leg fills]──→ Idle (arb complete)
                              │
                    [other leg rejected/expired/canceled]
                              │
                              ▼
                         Unwinding ──[unwind fills]──→ Idle (loss cut)
```

- **Idle**: no active orders on this pair. Arb detection runs on every quote update.
- **PendingEntry**: both leg orders submitted as GTD limit orders. Arb detection is
  suppressed for this pair to prevent duplicate entries.
- **PartialFill**: one leg has filled but the other is still open. The strategy waits
  for the second leg — if it also fills, the arb succeeds. If it fails, the strategy
  transitions to Unwinding.
- **Unwinding**: one leg filled, the other failed. The strategy submits an IOC limit
  order on the filled leg's instrument to exit the position at the current market price
  (adjusted for `unwind_slippage_bps`). Once the unwind fills, the pair returns to Idle.

#### Entry orders

When an arb is detected and `live_trading` is `true`:

1. **Guard checks**: no active arb on this pair, in-flight arbs < `max_concurrent_arbs`.
2. **Both legs submitted** as limit orders at the detected ask/bid prices.
   - `TimeInForce::Gtd` with `order_expire_secs` expiry — orders auto-cancel if not
     filled within the window.
   - `post_only = use_post_only` — for 0% maker fee when enabled.
3. **State transitions** to `PendingEntry`. Quote-driven arb detection is suppressed
   for this pair until the attempt resolves.

#### Unwind logic

If one leg fills but the other is rejected, expires, or is externally canceled:

1. The filled quantity and instrument are identified from the execution tracker.
2. An **IOC limit order** is submitted on the filled leg's instrument at the opposite
   side (sell back what was bought, or buy back what was sold).
3. The IOC price is set aggressively: current bid/ask widened by `unwind_slippage_bps`
   to maximize fill probability.
4. If the unwind fills, the loss is limited to slippage + fees. If the unwind itself
   fails (no liquidity), a critical error is logged and the position requires manual
   intervention.

#### Execution diagnostics

The strategy tracks four execution counters alongside the detection counters:

- **`arbs_submitted`**: total arb attempts with orders sent to the venue.
- **`arbs_completed`**: both legs filled successfully — profit captured.
- **`arbs_unwound`**: one leg failed, position was unwound — controlled loss.
- **`arbs_failed`**: both legs failed or unwind failed — no position or stuck position.

These appear in the periodic diagnostic summary log.

### Diagnostic tracking

The strategy maintains real-time diagnostics:

- **`quotes_processed`**: total quote evaluations across all pairs.
- **`buy_arbs_detected`** / **`sell_arbs_detected`**: arb opportunity counts.
- **`best_buy_spread`**: lowest combined ask seen (closest to buy arb).
- **`best_sell_spread`**: highest combined bid seen (closest to sell arb).

A diagnostic summary is logged every 500 quote evaluations:

```
SUMMARY | quotes=1500 buy_arbs=2 sell_arbs=0 submitted=1 completed=1 unwound=0 failed=0 | best_buy_spread=0.9823 (NBA Finals) best_sell_spread=0.9412 (World Cup)
```

## Data pipeline: how instruments reach the strategy

Understanding the data flow is key to configuring the strategy correctly.

### Instrument loading (startup)

```
Gamma Events API
  │
  ├── EventParamsFilter queries: GET /events?active=true&tag_slug=sports&liquidity_min=100000
  │
  ├── Returns events with markets → adapter fetches instrument details per market
  │
  └── BinaryOption instruments added to cache
        │
        └── Strategy.on_start() → discover_pairs() → subscribe_quotes()
```

The `EventParamsFilter` controls which markets are loaded at startup. By filtering on
`tag_slug`, `liquidity_min`, and `max_events`, you control the initial universe of pairs.

### Dynamic market discovery (runtime)

```
WebSocket (subscribe_new_markets: true)
  │
  ├── Polymarket publishes new market events
  │
  ├── NewMarketPredicateFilter checks tags → accepts/rejects
  │
  ├── Accepted → adapter fetches full instrument details → adds to cache
  │
  └── Strategy.on_instrument() → try_match_complement() → subscribes new quotes
```

When `subscribe_new_markets: true`, the adapter subscribes to a WebSocket channel that
pushes new market listings. The `NewMarketPredicateFilter` gates which markets are
accepted — in this example, only markets tagged with "sports" or "sport".

## Configuration

### Strategy parameters

| Parameter                 | Type              | Default       | Description                                                       |
|---------------------------|-------------------|---------------|-------------------------------------------------------------------|
| `venue`                   | `Venue`           | *required*    | Venue to scan for binary options (e.g. `POLYMARKET`).             |
| `client_id`               | `Option<ClientId>`| `None`        | Client ID for data subscriptions and order routing.               |
| `fee_estimate_bps`        | `Decimal`         | `0`           | Conservative fee estimate in basis points.                        |
| `min_profit_bps`          | `Decimal`         | `50`          | Minimum profit (bps) after fees to trigger arb. 50 = 0.5%.        |
| `min_profit_abs`          | `Decimal`         | `0`           | Minimum absolute dollar profit per arb (0 = disabled).            |
| `trade_size`              | `Decimal`         | `10`          | Number of shares per leg.                                         |
| `max_concurrent_arbs`     | `usize`           | `1`           | Maximum simultaneous in‑flight arbs across all pairs.             |
| `use_post_only`           | `bool`            | `true`        | Use post‑only orders for 0% maker fee.                            |
| `order_expire_secs`       | `u64`             | `15`          | GTD expiry for entry orders in seconds.                           |
| `unwind_slippage_bps`     | `Decimal`         | `50`          | Slippage tolerance for IOC unwind orders, in bps.                 |
| `live_trading`            | `bool`            | `false`       | Enable live order submission. False = detect‑only.                |

### Choosing parameters

**`fee_estimate_bps`**: The strategy uses Polymarket's per-leg fee curve:
`fee_per_leg = (fee_estimate_bps / 10_000) × p × (1 - p)`. This peaks at p=0.50 and
drops to zero at price extremes. Set to `0.0` for post-only orders (maker fee = 0% on
Polymarket), or `200.0` for taker orders (Polymarket's current 2% base rate).

**`min_profit_bps`**: 50 bps (0.5%) is a conservative starting point. On a 10-share
position at $0.50/share, that's $0.025 absolute profit.

**`min_profit_abs`**: Set a dollar floor to avoid chasing sub-cent arbs. For example,
`min_profit_abs = 0.50` requires at least $0.50 absolute profit per pair (computed as
`profit_per_share * trade_size`). Works alongside `min_profit_bps` — both gates must pass.
Default `0.0` disables this check.

**`trade_size`**: Start small (10 shares) to validate the pipeline, then scale up based
on observed liquidity. The strategy checks `ask_size`/`bid_size` against `trade_size`
before triggering.

**`live_trading`**: Defaults to `false` for safety — the strategy detects and logs arbs
without submitting orders. Set to `true` only after validating detection output on your
target markets. This two-mode design lets you tune `min_profit_bps`, `fee_estimate_bps`,
and `trade_size` in observation mode before committing capital.

**`order_expire_secs`**: Controls GTD expiry for entry limit orders. Default 15 seconds.
Shorter values reduce stale-order risk (arb may have evaporated by the time the order
fills) but increase the chance of both legs expiring before filling. For liquid markets,
10-15 seconds is reasonable. For illiquid markets, consider 30-60 seconds.

**`unwind_slippage_bps`**: When one leg fills but the other fails (partial fill), the
strategy submits an IOC (Immediate-or-Cancel) unwind order to exit the filled position.
The price is widened by this amount from the current bid/ask. Default 50 bps (0.5%).
Higher values increase fill probability but worsen the loss on failed arbs.

### Data client filter parameters

The `EventParamsFilter` wraps `GetGammaEventsParams` for controlling which markets load
at startup:

| Parameter        | Type             | Description                                           |
|------------------|------------------|-------------------------------------------------------|
| `active`         | `Option<bool>`   | Only active (non‑resolved) markets.                   |
| `closed`         | `Option<bool>`   | Exclude closed markets.                               |
| `tag_slug`       | `Option<String>` | Filter by category tag (e.g. `"sports"`, `"politics"`).|
| `liquidity_min`  | `Option<f64>`    | Minimum liquidity in USD.                             |
| `liquidity_max`  | `Option<f64>`    | Maximum liquidity in USD.                             |
| `volume_min`     | `Option<f64>`    | Minimum volume in USD.                                |
| `max_events`     | `Option<u32>`    | Client‑side cap on number of events to load.          |
| `limit`          | `Option<u32>`    | Server‑side page size.                                |
| `offset`         | `Option<u32>`    | Server‑side pagination offset.                        |

### Polymarket execution client

| Parameter          | Type            | Default                | Description                                        |
|--------------------|-----------------|------------------------|----------------------------------------------------|
| `trader_id`        | `TraderId`      | `TraderId::default()`  | Trader identifier.                                 |
| `account_id`       | `AccountId`     | `"POLYMARKET-001"`     | Account identifier.                                |
| `signature_type`   | `SignatureType`  | `Eoa`                  | Signing method: `Eoa`, `PolyProxy`, or `PolyGnosisSafe`. |
| `http_timeout_secs`| `u64`           | `60`                   | HTTP request timeout.                              |
| `max_retries`      | `u32`           | `3`                    | Maximum retry attempts for failed requests.        |
| `ack_timeout_secs` | `u64`           | `5`                    | Timeout waiting for order acknowledgement.         |

## Code walkthrough

This section walks through the complete `main()` function from `complement_arb.rs`,
connecting each block back to the concepts covered earlier.

### Instrument universe: startup filters

The first thing to configure is *which markets the strategy will see*. This maps directly
to the [data pipeline](#data-pipeline-how-instruments-reach-the-strategy) described above —
the `EventParamsFilter` controls the Gamma API query that populates the instrument cache
before `on_start()` runs pair discovery.

```rust
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();

    let environment = Environment::Live;
    let trader_id = TraderId::from("TESTER-001");
    let account_id = AccountId::from("POLYMARKET-001");
    let client_id = ClientId::new("POLYMARKET");
    let venue = Venue::new("POLYMARKET");

    // Startup filter: controls which instruments load into cache via the Gamma API.
    // The strategy's discover_pairs() in on_start() will only see instruments that
    // pass this filter. Here we target active sports markets with >$100k liquidity,
    // capped at 10 events to stay within WebSocket subscription limits.
    let sports_events = GetGammaEventsParams {
        active: Some(true),
        closed: Some(false),
        tag_slug: Some("sports".into()),
        liquidity_min: Some(100000.0),
        max_events: Some(10),
        ..Default::default()
    };
    let data_filter = EventParamsFilter::new(sports_events);
```

### Dynamic discovery: runtime filters

With `subscribe_new_markets: true`, the adapter pushes new market listings via WebSocket.
The `NewMarketPredicateFilter` gates which of those reach the strategy's `on_instrument()`
handler, where the O(1) complement matching logic tries to form new pairs at runtime.

```rust
    // Runtime filter: gates WebSocket new-market events before they reach the strategy.
    // Without this, every newly listed market (politics, crypto, pop culture) would
    // trigger on_instrument() and potentially subscribe quotes for irrelevant pairs.
    let new_market_filter = NewMarketPredicateFilter::new("sports-only", |nm| {
        nm.tags
            .iter()
            .any(|t| t.eq_ignore_ascii_case("sports") || t.eq_ignore_ascii_case("sport"))
    });

    let data_config = PolymarketDataClientConfig {
        subscribe_new_markets: true,       // enable WebSocket market discovery
        filters: vec![Arc::new(data_filter)],
        new_market_filter: Some(Arc::new(new_market_filter)),
        ..Default::default()
    };
```

### Execution client: signing and order routing

The execution client handles order submission, cancellation, and fill reporting. The
`signature_type` determines how orders are signed — this is relevant to the
[execution](#execution) section's entry and unwind orders, which both flow through this
client.

```rust
    // PolyGnosisSafe uses the browser wallet proxy signing mode.
    // Switch to SignatureType::Eoa for standard EOA signing with POLYMARKET_PK.
    let exec_config = PolymarketExecClientConfig {
        trader_id,
        account_id,
        signature_type: SignatureType::PolyGnosisSafe,
        ..Default::default()
    };
```

### Node assembly: reconciliation and lifecycle

The `LiveNode` ties data and execution together. Reconciliation is critical for the
state machine — on restart, the node queries the Polymarket REST API for open orders
and positions so the strategy doesn't submit duplicate arbs on pairs that already have
active entries. The `delay_post_stop_secs` grace period ensures that residual fill and
cancel events from the [unwind logic](#unwind-logic) are processed before shutdown.

```rust
    let log_config = LoggerConfig {
        stdout_level: LevelFilter::Info,
        ..Default::default()
    };

    let mut node = LiveNode::builder(trader_id, environment)?
        .with_name("POLYMARKET-COMPLEMENT-ARB-001".to_string())
        .with_logging(log_config)
        .add_data_client(
            None,
            Box::new(PolymarketDataClientFactory),
            Box::new(data_config),
        )?
        .add_exec_client(
            None,
            Box::new(PolymarketExecutionClientFactory),
            Box::new(exec_config),
        )?
        // Reconcile open orders/positions on startup — prevents the state machine
        // from treating existing positions as Idle and submitting duplicate entries.
        .with_reconciliation(true)
        .with_reconciliation_lookback_mins(120)
        .with_timeout_reconciliation(60)
        // Grace period for residual fill/cancel events during shutdown.
        .with_delay_post_stop_secs(5)
        .build()?;
```

### Strategy config: arb detection and execution thresholds

These parameters directly control the [buy/sell arb detection](#buy-arb-detection) gates
and the [per-pair state machine](#per-pair-state-machine) behavior. Note that
`fee_estimate_bps` defaults to `0.0` here because `use_post_only` defaults to `true` —
maker orders have 0% fee on Polymarket, so no fee adjustment is needed. The
`order_expire_secs` sets the GTD window for entry orders; if both legs expire, the pair
returns to Idle with no harm done.

All bps/quantity/dollar fields are `rust_decimal::Decimal`, so the `dec!` macro from
`rust_decimal_macros` is the cleanest way to pass literals:

```rust
use rust_decimal_macros::dec;

    let strategy_config = ComplementArbConfig::builder()
        .venue(venue)
        .client_id(client_id)
        .min_profit_bps(dec!(50))    // minimum 0.5% profit after fees to trigger
        .min_profit_abs(dec!(0.50))  // minimum $0.50 absolute profit per arb
        .trade_size(dec!(10))        // 10 shares per leg — checked against ask/bid size
        .live_trading(true)          // false = detection-only, no orders submitted
        .order_expire_secs(15)       // GTD expiry — state machine transitions on expiry
        .build();

    let strategy = ComplementArb::new(strategy_config);

    node.add_strategy(strategy)?;
    node.run().await?;

    Ok(())
}
```

## Event flow

```
LiveNode starts
  │
  ├── connect() → HTTP: load instruments via Gamma API (filtered by EventParamsFilter)
  │                WebSocket: subscribe market channels
  │
  ├── on_start()
  │     ├── discover_pairs() → match Yes/No pairs by condition ID
  │     ├── subscribe_quotes() for each pair leg
  │     └── subscribe_instruments(POLYMARKET) for new market discovery
  │
  ├── on_quote() [repeated for each pair leg]
  │     ├── Cache latest quote
  │     ├── Look up pair by instrument ID
  │     ├── Skip arb checks if pair has active execution
  │     ├── check_buy_arb(): yes_ask + no_ask < 1.0? → submit_arb() if live_trading
  │     ├── check_sell_arb(): yes_bid + no_bid > 1.0? → submit_arb() if live_trading
  │     └── Log diagnostic summary every 500 evaluations
  │
  ├── on_order_filled()
  │     ├── Update cumulative fill qty (avg fill price read from order cache via Order::avg_px())
  │     ├── Both legs filled? → ARB COMPLETE, cleanup
  │     ├── One leg filled, other open? → transition to PartialFill
  │     └── Unwind order filled? → UNWIND COMPLETE, cleanup
  │
  ├── on_order_rejected() / on_order_expired() / on_order_canceled()
  │     ├── Neither leg had fills? → cancel other leg, cleanup
  │     ├── One leg had fills? → initiate unwind (IOC exit order)
  │     └── Unwind order failed? → log critical error, cleanup
  │
  └── on_stop()
        ├── Cancel all active arb orders
        └── Log final execution summary
```

## Running the example

```bash
cargo run --example polymarket-complement-arb --package nautilus-polymarket
```

### Expected startup output

```
ComplementArb started: 14 pairs, trade_size=10, min_profit=50bps, fee=0bps, live=true
INSTRUMENT RECEIVED | 0xabc...111.POLYMARKET
NEW INSTRUMENT | Premier League Title - Manchester City | outcome=Yes | 0xabc...111.POLYMARKET
NEW INSTRUMENT | Premier League Title - Manchester City | outcome=No | 0xabc...222.POLYMARKET
Paired new complement: Premier League Title - Manchester City (total: 15)
```

### Expected steady-state output

```
SUMMARY | quotes=500 buy_arbs=0 sell_arbs=0 submitted=0 completed=0 unwound=0 failed=0 | best_buy_spread=0.9912 (NBA Finals Game 7) best_sell_spread=0.9534 (World Cup Winner)
BUY ARB | Premier League Title | profit=124.0bps | yes_ask=0.480 + no_ask=0.496 = 0.976 | fee=0.0000 | $profit=0.2400
ARB SUBMITTED | Premier League Title | side=Buy | profit=124.0bps | yes=O-20260403-001-001 no=O-20260403-001-002
ARB COMPLETE | 0xabc...def | yes_px=0.4800 no_px=0.4960
```

### Graceful shutdown

Press **Ctrl+C** to stop the node. The shutdown sequence:

1. SIGINT received, trader stops, `on_stop()` fires.
2. Strategy cancels all active arb orders.
3. Strategy logs final execution summary.
4. 5-second grace period (`delay_post_stop_secs`) processes residual events.
5. Clients disconnect, node exits.

## Monitoring and understanding output

### Key log messages

| Log message                                                   | Meaning                                              |
|---------------------------------------------------------------|------------------------------------------------------|
| `ComplementArb started: N pairs, ...`                         | Strategy initialized with N complement pairs.        |
| `INSTRUMENT RECEIVED \| <id>`                                 | New instrument arrived from venue.                   |
| `NEW INSTRUMENT \| <description> \| outcome=<Yes/No> \| <id>` | Runtime instrument discovery picked up a new market. |
| `Paired new complement: <label> (total: M)`                   | Runtime discovery matched a Yes/No pair.             |
| `BUY ARB \| <label> \| profit=Xbps \| ... \| $profit=Y`       | Buy arb detected with absolute profit Y.             |
| `SELL ARB \| <label> \| profit=Xbps \| ... \| $profit=Y`      | Sell arb detected with absolute profit Y.            |
| `BUY ARB \| <label> \| skipped: insufficient ...`             | Arb found but liquidity too thin for trade_size.     |
| `ARB SUBMITTED \| <label> \| side=X \| ...`                   | Both leg orders sent to venue.                       |
| `ARB COMPLETE \| <label> \| yes_px=X no_px=Y`                 | Both legs filled — arb profit locked in.             |
| `ARB LEG <reason> \| <pair_key> \| canceling other leg ...`   | One leg rejected/expired/canceled with no fills — strategy cancels its sibling. |
| `PARTIAL FILL \| <label> \| one leg closed...`                | One leg filled, other still pending — at risk.      |
| `UNWIND SUBMITTED \| pair=<key> \| <id> \| ...`               | IOC unwind order sent for a partial‑fill leg.        |
| `UNWIND COMPLETE \| <label> \| filled @ X`                    | Unwind order filled — loss controlled.               |
| `UNWIND FAILED \| <label> \| ...`                             | Unwind order failed — manual intervention needed.    |
| `SUMMARY \| ... submitted=N completed=M unwound=X failed=Y`   | Execution counters in periodic summary.              |
| `Order rejected: <id> — <reason>`                             | Order rejected by venue.                             |

### Understanding the diagnostics

The `best_buy_spread` and `best_sell_spread` fields show how close the market has come to
an arb opportunity:

- **`best_buy_spread` approaching 1.0 from below**: the market is near-efficient on the
  buy side. Values like `0.998` mean the combined ask is only 20 bps from arb.
- **`best_sell_spread` approaching 1.0 from above**: similar for sell side. Values like
  `1.002` mean the combined bid is only 20 bps from arb.
- **`best_buy_spread` well below 1.0** (e.g. `0.95`): large structural mispricing or
  illiquid market with wide spreads.

### Troubleshooting execution counters

If your summary line shows unexpected values, use the counters to narrow the problem:

- **`arbs_failed` > 0, `unwound` = 0**: both legs are failing before either fills. Look
  for `Order rejected` messages in the log — common causes are insufficient USDC.e
  allowance, expired API credentials, or `trade_size` exceeding the venue's minimum/maximum
  order size. If rejections mention "post-only", the market may have moved and your limit
  price would cross the spread; try setting `use_post_only: false` or reducing
  `order_expire_secs`.
- **`unwound` high relative to `completed`**: one leg fills consistently but the other
  doesn't. This usually means the arb is closing between the first and second leg
  submission — the market is faster than your execution. Consider raising `min_profit_bps`
  to only trigger on wider spreads that survive the submission latency, or reduce
  `order_expire_secs` to fail faster and limit exposure.
- **`UNWIND FAILED` in logs**: the IOC exit order found no liquidity on the filled leg's
  side. The position is now stuck and requires manual intervention. Check the instrument's
  order book depth — if the book is empty, the market may be illiquid or near resolution.
  Raise `liquidity_min` in `EventParamsFilter` to avoid thin markets, or increase
  `unwind_slippage_bps` to price the exit more aggressively.
- **`submitted` > 0 but `completed` + `unwound` + `failed` = 0**: arb orders are in
  flight but nothing has resolved yet. This is normal during the first few seconds. If it
  persists, check that the WebSocket feed is delivering fill and cancel events — a stalled
  connection will leave the state machine stuck in `PendingEntry`.

## Risk considerations

### Execution risks

- **Non-atomic execution**: Polymarket does not support atomic cross-instrument orders.
  The strategy submits both legs as fast as possible, but there is inherent latency
  between submissions. Detection-to-fill latency matters — the CLOB is
  first-come-first-served. Use `live_trading: false` to validate detection before
  enabling execution.
- **Partial fill risk**: because the two legs are submitted independently, one may fill
  while the other is rejected or expires. The strategy automatically unwinds partial fills
  via IOC exit orders, but the unwind price may be worse than the entry price, resulting
  in a small loss. The `unwind_slippage_bps` parameter controls how aggressively the
  unwind is priced.
- **Unwind failure**: if the IOC unwind order cannot fill (empty book on that side), the
  strategy logs a critical error and the position must be closed manually. Monitor the
  `arbs_failed` counter and `UNWIND FAILED` log messages.

### Market risks

- **Quote staleness**: the strategy evaluates arbs using the latest cached quote per
  instrument. If one leg's quote is stale (e.g. the WebSocket connection lagged), the
  detected spread may not reflect the live market.
- **Liquidity depth**: the strategy checks top-of-book `ask_size`/`bid_size` against
  `trade_size`, but the posted liquidity may be pulled or stale. Large `trade_size`
  values may not fill at the quoted price.
- **Market resolution**: binary options settle to 0 or 1. Holding both sides until
  resolution guarantees the $1 payout. Early exit requires finding liquidity on both
  sides, which may not exist for illiquid markets.

### Operational risks

- **Fee curve accuracy**: the strategy uses Polymarket's per-leg fee formula
  `(fee_estimate_bps / 10_000) × p × (1 - p)`, which peaks at p=0.50 and drops to zero at
  extremes. Set `fee_estimate_bps` to match the venue's base fee rate: `200.0` for taker
  orders (2% on Polymarket), or `0.0` with `use_post_only: true` for maker orders (0% fee).
- **GTD expiry tuning**: too-short expiry increases the chance of both legs expiring
  (no harm, but missed opportunities). Too-long expiry risks filling a stale arb where
  the spread has closed. 15 seconds is a reasonable default.
- **WebSocket subscription limits**: Polymarket limits the number of concurrent WebSocket
  subscriptions. Loading too many markets via `EventParamsFilter` can exhaust this limit.
  Use `max_events` and `liquidity_min` to control the universe size.

## Customization tips

### Filtering by category

Change the `tag_slug` to target different market categories:

```rust
let events = GetGammaEventsParams {
    tag_slug: Some("politics".into()),  // or "crypto", "pop-culture", etc.
    liquidity_min: Some(50000.0),
    max_events: Some(20),
    ..Default::default()
};
```

### Adjusting sensitivity

| Goal                        | Adjustment                                                     |
|-----------------------------|----------------------------------------------------------------|
| Catch more arbs             | Lower `min_profit_bps` (e.g. 10 = 0.1%).                      |
| Reduce false positives      | Raise `min_profit_bps` (e.g. 100 = 1.0%).                     |
| Set absolute profit floor   | Use `.min_profit_abs(0.50)` to require $0.50/arb.              |
| Trade larger size           | Increase `trade_size` (check liquidity first).                 |
| Cover more markets          | Raise `max_events`, lower `liquidity_min`.                     |
| Reduce WebSocket load       | Lower `max_events`, raise `liquidity_min`.                     |
| Accept all market categories| Remove `tag_slug` filter and `NewMarketPredicateFilter`.       |
| Enable execution            | Set `live_trading(true)`.                                      |
| Wider unwind tolerance      | Increase `unwind_slippage_bps` for better fill probability.    |
| Faster order expiry         | Lower `order_expire_secs` (e.g. 10s for liquid markets).      |

### Disabling dynamic discovery

If you only want to monitor markets loaded at startup (no new market subscriptions):

```rust
let data_config = PolymarketDataClientConfig {
    subscribe_new_markets: false,  // no WebSocket market discovery
    filters: vec![Arc::new(data_filter)],
    new_market_filter: None,
    ..Default::default()
};
```

The strategy's `subscribe_instruments` call in `on_start` will have no effect without
new market events flowing from the adapter.

## What's next

### Validate before risking capital

Run in detection-only mode (`live_trading: false`) for at least 24 hours. Check the
diagnostic summary: if no arbs are detected, lower `min_profit_bps` to 10 (0.1%) to
see what exists, then tune upward. Frequent "skipped: insufficient liquidity" means
`trade_size` is too large for available depth. If `best_buy_spread` stays above 0.99,
arbs are rare on those markets — widen the universe.

### Backtesting

Record `QuoteTick` streams using NautilusTrader's data catalog, then swap `LiveNode`
for `BacktestNode` to replay them through the same arb detection logic. Compare
`arbs_detected` vs. `arbs_completed` for theoretical capture rate. Note that historical
quotes don't reflect fill competition — treat results as an upper bound. See the
[backtesting guide](../concepts/backtesting.md) for setup.

### Improving execution

Detection is the easy part — alpha loss is in execution.

- **Sequential legs.** Submit the less liquid leg first; once filled, submit the second.
  Eliminates partial fills at the cost of missing arbs where the second leg moves.
- **Adaptive pricing.** Submit slightly above ask / below bid (e.g. +0.001) to increase
  fill priority. Add a `price_aggression_bps` parameter.
- **Quote staleness guard.** Reject arb signals where either leg's `ts_event` is older
  than a threshold (500ms–1s). Prevents phantom arbs from a lagging WebSocket.
- **Unwind retries.** Retry with progressively wider slippage instead of treating a
  single IOC failure as terminal.

### Expanding the strategy

- **Order book depth.** Subscribe to `OrderBookDeltas` to walk the book on both legs
  and compute the exact maximum arb size where cumulative cost stays below 1.0 — this
  replaces the fixed `trade_size` parameter entirely.
- **Multi-venue arb.** Match Yes on one venue against No on another by event description
  rather than condition ID.
- **Categorical markets.** Markets with 3+ outcomes generalize the constraint: all prices
  must sum to 1.0. Extend `discover_pairs` to handle arbitrary outcome counts.
- **Taker mode.** IOC orders trade fee cost for fill certainty. Auto-adjust the threshold:
  `effective_min_profit = min_profit_bps + 2 × taker_fee_bps`.

### Risk controls for production

- **Daily loss limit.** Track cumulative P&L from unwinds; pause trading above a threshold.
- **Circuit breaker.** Pause submissions if `arbs_failed` spikes (e.g. 3 in 5 minutes).
- **Per-pair cooldown.** Skip a pair for a configurable period after an unwind or failure.
