# Delta-Neutral Volatility

Short volatility hedger for externally held OTM strangles, with delta hedging
via the underlying perpetual swap.

## Strategy overview

The strategy assumes an out-of-the-money call and an out-of-the-money put on
the same underlying and expiry (a short strangle) already exist in the
account. The combined position starts near delta-neutral because the call
and put deltas roughly offset. As the underlying moves, the net delta drifts
away from zero. The strategy monitors this drift and rehedges by trading the
underlying perpetual swap whenever the portfolio delta exceeds a
configurable threshold.

### Entry

On startup the strategy:

1. Queries the instrument cache for all options matching the configured
   `option_family` (e.g. `BTC-USD`).
2. Filters to the nearest expiry (or a specific expiry via `expiry_filter`).
3. Selects a call and put using a strike-percentile heuristic derived from
   `target_call_delta` and `target_put_delta`.
4. Hydrates any existing positions from the cache (reconciliation may have
   loaded them from a previous session).
5. Subscribes to venue-provided Greeks for both legs and quotes for the
   hedge instrument.
6. Waits for Greeks from both option legs before enabling rehedging.

When `enter_strangle` is `true` (the default) and no option positions exist,
the strategy places SELL limit orders for both legs once the first Greeks
updates arrive. Orders are priced in implied volatility (`px_vol`) at the
mark IV minus `entry_iv_offset`. An offset of 0.0 sells at mark; a positive
offset (e.g. 0.02) sells 2 vol points below mark for faster fills.

Set `entry_premium_offset_ticks` to use explicit option premium prices
instead of IV params. In that mode the strategy subscribes to quotes for both
option legs and sells at the current ask plus the configured tick offset. If
the quote side is empty, it computes a premium from the venue IV fields.

When `enter_strangle` is `false` the strategy operates as a delta hedger
for positions entered externally or carried forward from a previous session.

### Rehedging

The strategy rehedges on two triggers:

- **Greeks update**: every time `on_option_greeks` fires, the strategy
  recomputes portfolio delta and submits a hedge if the threshold is
  breached. Rehedging starts only after both option legs have produced a
  Greeks update.
- **Periodic timer**: a safety net that checks portfolio delta on a fixed
  interval (`rehedge_interval_secs`), catching cases where greeks updates
  stop arriving.

A `hedge_pending` guard prevents duplicate hedge submissions while a prior
hedge order is still in flight.

### Position tracking

Positions are tracked via `on_order_filled`. Each fill on the hedge
instrument, call leg, or put leg updates the corresponding position field.
Portfolio delta is computed as:

```
portfolio_delta = call_delta * call_position
                + put_delta * put_position
                + hedge_position
```

### Exit

On stop the strategy cancels open hedge orders and unsubscribes from all
data feeds. It leaves live positions unchanged.

## Configuration

| Parameter                    | Type             | Default    | Description                                     |
|------------------------------|------------------|------------|-------------------------------------------------|
| `option_family`              | `String`         | *required* | Option family filter, e.g. `BTC-USD`.           |
| `hedge_instrument_id`        | `InstrumentId`   | *required* | Underlying hedge instrument.                    |
| `client_id`                  | `ClientId`       | *required* | Data and execution client identifier.           |
| `target_call_delta`          | `f64`            | `0.20`     | Startup call strike heuristic target.           |
| `target_put_delta`           | `f64`            | `-0.20`    | Startup put strike heuristic target.            |
| `contracts`                  | `u64`            | `1`        | Number of option contracts per leg.             |
| `rehedge_delta_threshold`    | `f64`            | `0.5`      | Absolute portfolio delta that triggers a hedge. |
| `rehedge_interval_secs`      | `u64`            | `30`       | Periodic rehedge timer interval in seconds.     |
| `expiry_filter`              | `Option<String>` | `None`     | Restrict to a specific expiry, e.g. `260327`.   |
| `enter_strangle`             | `bool`           | `true`     | Place strangle entry orders.                    |
| `entry_iv_offset`            | `f64`            | `0.0`      | Vol points subtracted from mark IV.             |
| `entry_time_in_force`        | `TimeInForce`    | `Gtc`      | Time-in-force for strangle entry orders.        |
| `entry_premium_offset_ticks` | `Option<i32>`    | `None`     | Ticks above ask for explicit premium entry.     |

### Inherited from `StrategyConfig`

The `base` field carries the standard strategy configuration. The most
relevant fields:

- `strategy_id`: defaults to `DELTA_NEUTRAL_VOL-001`.
- `order_id_tag`: defaults to `001`. Set to a unique value when running
  multiple instances.
- `use_uuid_client_order_ids`: set to `true` to avoid ID collisions across
  restarts.

## Risk considerations

- **Gamma risk**: a short strangle has negative gamma. Large underlying
  moves increase the delta exposure faster than the rehedge timer can
  respond. Tighten `rehedge_delta_threshold` and reduce
  `rehedge_interval_secs` for faster response, at the cost of higher
  transaction costs.
- **Vega risk**: a spike in implied volatility increases the mark-to-market
  loss on the short options. The strategy does not manage vega exposure
  directly.
- **Lifecycle risk**: stopping the strategy disables further hedge updates.
  It does not unwind either the options or the hedge. Any exit must be
  managed separately.
- **Liquidity**: OTM options on crypto venues can have wide spreads and
  empty bid/ask arrays. The adapter's `QuoteCache` handles partial BBO
  updates, and hedge quality can degrade when the market gaps or the hedge
  instrument trades in coarse size increments.
- **Contract multipliers**: the portfolio delta computation uses raw
  contract counts. For instruments with non-unit multipliers (OKX inverse
  options), the hedge quantity may need scaling by the contract value.
  A production deployment should account for this.

## Rust usage

```rust
use nautilus_trading::examples::strategies::{DeltaNeutralVol, DeltaNeutralVolConfig};

let config = DeltaNeutralVolConfig::builder()
    .option_family("BTC-USD".to_string())
    .hedge_instrument_id(InstrumentId::from("BTC-USD-SWAP.OKX"))
    .client_id(ClientId::new("OKX"))
    .target_call_delta(0.25)
    .target_put_delta(-0.25)
    .contracts(5)
    .rehedge_delta_threshold(0.3)
    .rehedge_interval_secs(15)
    .expiry_filter("260627".to_string())
    .entry_iv_offset(0.02) // Sell 2 vol points below mark
    .build();

let strategy = DeltaNeutralVol::new(config);
node.add_strategy(strategy)?;
```

## Python usage (v2)

Pass the config to `add_builtin_strategy` on a `LiveNode` or
`BacktestEngine`. Python provides the configuration; the strategy
runs entirely in Rust.

```python
from nautilus_trader.core.nautilus_pyo3.trading import DeltaNeutralVolConfig

config = DeltaNeutralVolConfig(
    option_family="BTC-USD",
    hedge_instrument_id=InstrumentId.from_str("BTC-USD-SWAP.OKX"),
    client_id=ClientId("OKX"),
    target_call_delta=0.25,
    target_put_delta=-0.25,
    contracts=5,
    rehedge_delta_threshold=0.3,
    rehedge_interval_secs=15,
    expiry_filter="260627",
    entry_iv_offset=0.02,  # Sell 2 vol points below mark
)

node.add_builtin_strategy("DeltaNeutralVol", config)
```
