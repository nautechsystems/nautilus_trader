# Complement Arbitrage

Binary option complement arbitrage strategy that exploits pricing
inefficiencies in Yes/No market pairs.

For an end-to-end walkthrough — event flow, state machine, live order
submission, partial-fill unwinding, monitoring, and tuning — see the
[Polymarket complement arb tutorial](../../../../../../docs/tutorials/complement_arb_polymarket.md).

## Strategy overview

Binary options have a mathematical complement constraint: at resolution,
`Yes + No = 1.0`. When market inefficiencies cause the combined ask
prices to fall below 1.0 (after fees), buying both sides locks in
risk-free profit. When combined bid prices exceed 1.0, selling both
sides does the same. The strategy continuously monitors all discovered
complement pairs and, when `live_trading = true`, submits both legs as
GTD limit orders.

### Pair discovery

On startup the strategy queries the instrument cache for all
`BinaryOption` instruments on the configured `venue`, groups them by
condition ID (the symbol prefix before the last `-`), and pairs
instruments that share a condition ID and have complementary `Yes` /
`No` outcomes. It then subscribes to quotes for both legs and to new
instrument events on the venue, so newly listed markets are paired
dynamically via `on_instrument` (`try_match_complement`).

### Detection

For every quote update, the strategy evaluates both sides of the pair:

```
combined_ask = yes_ask + no_ask        # buy arb
combined_bid = yes_bid + no_bid        # sell arb
fee          = combined_* * fee_estimate_bps / 10_000
profit_bps   = (1.0 - combined_ask - fee) * 10_000   # buy
profit_bps   = (combined_bid - fee - 1.0) * 10_000   # sell
```

An arb is actionable when `profit_bps >= min_profit_bps`,
`profit_per_share * trade_size >= min_profit_abs`, and the relevant
side of the book has at least `trade_size` of liquidity on both legs.

### Execution

When `live_trading = true` and an arb is actionable, the strategy
submits both legs as GTD limit orders (post-only by default) and
transitions the pair through a small state machine:

```
Idle → PendingEntry → (both fill) → ARB COMPLETE → Idle
                   → (one fills, other fails) → Unwinding → Idle
```

If one leg fills but the other is rejected, expires, or is canceled,
the strategy submits an IOC unwind order on the filled leg's instrument
to exit the position (priced aggressively using `unwind_slippage_bps`).
A second guard, `max_concurrent_arbs`, bounds the total in-flight arbs
across all pairs (per-pair concurrency is always 1).

When `live_trading = false`, the strategy detects and logs arbs but
submits no orders — useful for tuning thresholds before committing
capital.

### Diagnostics

The strategy maintains detection counters
(`quotes_processed`, `buy_arbs_detected`, `sell_arbs_detected`,
`best_buy_spread`, `best_sell_spread`) and execution counters
(`arbs_submitted`, `arbs_completed`, `arbs_unwound`, `arbs_failed`),
and emits a periodic `SUMMARY` log every 500 quote evaluations.

### Exit

`on_stop` cancels all active arb orders (entry and unwind legs) and
logs final detection + execution counts. It does not forcibly unwind
filled positions; that's the job of the runtime unwind path.

## Configuration

| Parameter             | Type               | Default     | Description                                                           |
|-----------------------|--------------------|-------------|-----------------------------------------------------------------------|
| `venue`               | `Venue`            | *required*  | Venue to scan for binary option instruments (e.g. `POLYMARKET`).      |
| `client_id`           | `Option<ClientId>` | `None`      | Client ID for data subscriptions and order routing.                   |
| `fee_estimate_bps`    | `Decimal`          | `0`         | Conservative fee estimate in basis points. 0 = no fee adjustment.     |
| `min_profit_bps`      | `Decimal`          | `50`        | Minimum profit (bps) after fees to trigger arb (50 = 0.5%).           |
| `min_profit_abs`      | `Decimal`          | `0`         | Minimum absolute dollar profit per arb (0 = disabled).                |
| `trade_size`          | `Decimal`          | `10`        | Number of shares per leg.                                             |
| `max_concurrent_arbs` | `usize`            | `1`         | Max simultaneous in-flight arbs across all pairs (per-pair = 1).      |
| `use_post_only`       | `bool`             | `true`      | Submit entry orders post-only (0% maker fee on Polymarket).           |
| `order_expire_secs`   | `u64`              | `15`        | GTD expiry for entry limit orders, in seconds.                        |
| `unwind_slippage_bps` | `Decimal`          | `50`        | Slippage tolerance for IOC unwind orders, in bps.                     |
| `live_trading`        | `bool`             | `false`     | Enable live order submission. False = detection-only.                 |

### Inherited from `StrategyConfig`

The `base` field carries the standard strategy configuration. The most
relevant fields:

- `strategy_id`: defaults to `COMPLEMENT_ARB-001`.
- `order_id_tag`: defaults to `001`. Set to a unique value when running
  multiple instances.

## Risk considerations

- **Fee model**: the flat `fee_estimate_bps` is a simplification.
  Polymarket uses a price-dependent fee curve that varies per leg. A
  production deployment should use the venue's actual fee formula.
- **Execution risk**: detection and order submission are not atomic.
  Between detection and fill, prices can move, eliminating the arb.
- **Partial fills**: if one leg fills but the other does not, the
  strategy unwinds via an IOC order; loss is bounded by
  `unwind_slippage_bps` plus fees, but unwind itself can fail in thin
  books and require manual intervention.
- **Liquidity**: binary option books can be thin. The strategy checks
  `ask_size`/`bid_size` against `trade_size` before acting, but posted
  liquidity can be stale.
- **Quote staleness**: the strategy uses the latest cached quotes. If
  one leg's quote is stale, the detected spread may not reflect the
  live market.

## Rust usage

```rust
use nautilus_trading::examples::strategies::{ComplementArb, ComplementArbConfig};
use rust_decimal_macros::dec;

let config = ComplementArbConfig::builder()
    .venue(Venue::new("POLYMARKET"))
    .client_id(ClientId::new("POLYMARKET"))
    .min_profit_bps(dec!(50))
    .trade_size(dec!(10))
    .live_trading(false)        // detection-only until validated
    .order_expire_secs(15)
    .build();

let strategy = ComplementArb::new(config);
node.add_strategy(strategy)?;
```
