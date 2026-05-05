# Composite Market Maker

Single-pair market making strategy that quotes around a target instrument's
book mid, with skew driven by current inventory and by an external signal
price (typically a `SyntheticInstrument`).

## Strategy overview

The strategy subscribes to quotes for two instruments: the target instrument
that it quotes on, and a signal instrument that drives the signal skew. Any
instrument that publishes quotes works as the signal source, but a
`SyntheticInstrument` is the typical choice because it lets the user encode a
multi-leg composite signal as a formula and reuse it across strategies.

One resting bid and one resting ask sit around the target's book mid, shifted
by inventory skew and signal skew. Orders persist across ticks and are only
replaced when either the anchor or the signal residual's price impact
(`signal_skew_factor * residual`) moves by at least `requote_threshold_bps`
of the anchor. This keeps cancel/replace traffic proportional to real price
moves while still picking up signal updates between target ticks.

### Quoting cycle

Each target instrument quote tick triggers the following:

1. Compute the anchor as the mid-price of the latest quote.
2. Skip the requote if both the anchor and the signal residual's price impact
   have moved less than `requote_threshold_bps` since the last placement.
3. Cancel all existing orders on the target instrument.
4. Read the current net position and worst-case per-side exposure (open
   positions plus all pending buy or sell orders) from the cache.
5. Compute bid and ask prices, apply inventory and signal skew, and enforce
   `max_position` before submitting each side.
6. Submit surviving sides as `post_only` limit orders.

Signal instrument quotes update an internal `last_signal` value passively and
never trigger requoting on their own. The next target tick reads the latest
signal value, and a sufficiently large change in the residual's price impact
will pull through the requote gate even when the anchor has not moved.

### Inventory skew

```
inventory_shift = inventory_skew_factor * net_position
```

When `inventory_skew_factor` is positive and the strategy is long, both sides
shift down: the bid moves further from the market and the ask moves closer.
The shift is symmetric, so the quoted spread is preserved; what changes is
where the spread sits relative to the anchor.

### Signal skew

```
residual = (signal_mid - baseline) / baseline
signal_shift = signal_skew_factor * residual
```

When `signal_skew_factor` is positive and the residual is positive, both
sides lift equally, which is what you want if the signal anticipates upward
drift in the target. The baseline is either the configured `signal_baseline`
(deterministic for backtests) or the first observed signal mid when unset.

### Position limits

Before submitting each side, the strategy projects the worst-case per-side
exposure: open positions plus all pending buy or sell orders. Sides that
would breach `max_position` are dropped. This holds the cap even while async
cancels are in flight, because pending orders are still counted as exposure
until the venue confirms the cancel.

The strategy stops adding at the cap, but does not actively reduce inventory.
Skew makes one side more attractive at the cap, and that side trading the
position back down is the only exit path.

## Configuration

| Parameter               | Type                 | Default    | Description                                                                          |
|-------------------------|----------------------|------------|--------------------------------------------------------------------------------------|
| `instrument_id`         | `InstrumentId`       | *required* | Target instrument the strategy quotes on.                                            |
| `signal_instrument_id`  | `InstrumentId`       | *required* | Signal instrument (typically a synthetic) whose mid drives the signal residual.      |
| `max_position`          | `Quantity`           | *required* | Hard cap on net exposure (long or short).                                            |
| `trade_size`            | `Option<Quantity>`   | `None`     | Size per quote. When `None`, resolves from the instrument's `min_quantity`.          |
| `half_spread_bps`       | `u32`                | `5`        | Half the desired quoted spread, in basis points of the anchor.                       |
| `inventory_skew_factor` | `f64`                | `0.0`      | Price units per unit of net position. Both sides shift down by this times position.  |
| `signal_skew_factor`    | `f64`                | `0.0`      | Price units per unit of normalized signal residual. Both sides shift up.             |
| `signal_baseline`       | `Option<f64>`        | `None`     | Baseline price for the signal residual. When `None`, captured from the first signal. |
| `requote_threshold_bps` | `u32`                | `5`        | Minimum anchor or signal-residual price-impact move in bps before re-quoting.        |
| `expire_time_secs`      | `Option<u64>`        | `None`     | Order expiry in seconds. When set, orders use GTD time-in-force.                     |
| `on_cancel_resubmit`    | `bool`               | `false`    | Resubmit on the next quote after an external cancel.                                 |

### Tuning guidelines

- **Liquid book**: a small `half_spread_bps` (5 to 10) with a tight
  `requote_threshold_bps` (2 to 5). The strategy captures spread but has to
  keep up with mid moves.
- **Illiquid book**: wider `half_spread_bps` (20 to 50) and
  `requote_threshold_bps` (10 to 20). Quotes absorb stale book conditions
  without churning cancels on every flicker.
- **Inventory control**: pick `inventory_skew_factor` so that
  `factor * max_position` lands at 10 to 50 percent of the half-spread.
  Above that, skewed quotes start crossing the live book and get rejected as
  post-only violations; below that, the skew is too small to change fill
  probabilities.
- **Signal weighting**: pick `signal_skew_factor` so that
  `factor * typical_residual` is a small multiple of the tick size. A
  signal that swings 5 percent off baseline with a `signal_skew_factor` of
  `1.0` produces a 0.05 unit shift, which is meaningful at low-priced
  instruments and invisible at high-priced ones.

## Rust usage

```rust
use nautilus_model::{identifiers::InstrumentId, types::Quantity};
use nautilus_trading::examples::strategies::{
    CompositeMarketMaker, CompositeMarketMakerConfig,
};

let config = CompositeMarketMakerConfig::new(
    InstrumentId::from("OCPI-H100-PERP.AX"),
    InstrumentId::from("SEMI-COMPOSITE.SYNTH"),
    Quantity::from("100"),
)
.with_trade_size(Quantity::from("100"))
.with_half_spread_bps(25)
.with_inventory_skew_factor(0.0005)
.with_signal_skew_factor(0.5)
.with_requote_threshold_bps(10);

let strategy = CompositeMarketMaker::new(config);
node.add_strategy(strategy)?;
```

## Python usage (v2)

Pass the config to `add_native_strategy` on a `LiveNode` or `BacktestEngine`.
Python provides the configuration; the strategy runs entirely in Rust.

```python
from nautilus_trader.core.nautilus_pyo3.trading import CompositeMarketMakerConfig

config = CompositeMarketMakerConfig(
    instrument_id=InstrumentId.from_str("OCPI-H100-PERP.AX"),
    signal_instrument_id=InstrumentId.from_str("SEMI-COMPOSITE.SYNTH"),
    max_position=Quantity.from_str("100"),
    trade_size=Quantity.from_str("100"),
    half_spread_bps=25,
    inventory_skew_factor=0.0005,
    signal_skew_factor=0.5,
    requote_threshold_bps=10,
)

node.add_native_strategy(config)
```
