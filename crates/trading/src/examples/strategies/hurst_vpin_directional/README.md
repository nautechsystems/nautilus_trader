# Hurst/VPIN Directional

A directional example strategy that combines a Hurst-exponent regime filter
on dollar bars with a VPIN (Volume-synchronized Probability of Informed
Trading) signal from trade aggressor flow. Entry timing is gated on the
live quote stream.

## Strategy overview

The strategy runs three concurrent pipelines over a single instrument:

1. **Per trade**: aggressive buy and aggressive sell volume is accumulated
   into the current dollar-bar bucket, using `TradeTick::aggressor_side`.
2. **Per bar** (bucket close): the bar's log return joins a rolling Hurst
   window, the bucket's signed and absolute imbalance joins a rolling VPIN
   window, accumulators reset, and the Hurst and VPIN signals are
   re-estimated.
3. **Per quote**: when the signals are warm and the strategy is flat, a
   trending Hurst and an informed VPIN of consistent sign trigger an IOC
   market order in the direction of the signed imbalance.

Exit fires from the bar pipeline on regime decay (Hurst drops below the
exit threshold) and from the quote pipeline on a holding-time cap.

### Signals

- **Hurst exponent** by rescaled range over `hurst_window` dollar-bar log
  returns, regressed across the lag set in `hurst_lags`. Values above
  `hurst_enter` indicate a persistent (trending) regime; values below
  `hurst_exit` indicate regime decay.
- **VPIN** defined on dollar-bar buckets. For each completed bar with
  nonzero aggressor-classified volume:

  ```
  imbalance = (buy_volume - sell_volume) / (buy_volume + sell_volume)
  ```

  VPIN is the mean of `|imbalance|` over the last `vpin_window` buckets.
  A signed variant retains the sign of `buy - sell` and carries the
  net informed direction.

### Sampling frame

Dollar bars (`VALUE` aggregation) are the natural sampling frame for both
signals, following Lopez de Prado (*Advances in Financial Machine
Learning*, Chapter 2). Each bar closes after a fixed notional has traded,
so the sampling frame adapts to market activity, and VPIN volume buckets
coincide with the bar boundaries.

## Parameters

| Parameter          | Type             | Default          | Description                                             |
|--------------------|------------------|------------------|---------------------------------------------------------|
| `instrument_id`    | `InstrumentId`   | required         | Instrument to subscribe to and trade.                   |
| `bar_type`         | `BarType`        | required         | Dollar bar type (`VALUE` aggregation, `LAST` prices).   |
| `trade_size`       | `Quantity`       | required         | Order quantity for each entry.                          |
| `hurst_window`     | `usize`          | `128`            | Rolling window of dollar bar log returns.               |
| `hurst_lags`       | `Vec<usize>`     | `[4, 8, 16, 32]` | Lag set used in the R/S regression.                     |
| `hurst_enter`      | `f64`            | `0.55`           | Above this, the regime is treated as trending.          |
| `hurst_exit`       | `f64`            | `0.50`           | Below this, open positions are flattened.               |
| `vpin_window`      | `usize`          | `50`             | Number of volume buckets averaged for VPIN.             |
| `vpin_threshold`   | `f64`            | `0.30`           | Minimum VPIN for flow to be considered informed.        |
| `max_holding_secs` | `u64`            | `3600`           | Maximum seconds a position may be held.                 |

## Rust usage

```rust
use nautilus_model::{
    data::BarType,
    identifiers::InstrumentId,
    types::Quantity,
};
use nautilus_trading::examples::strategies::{
    HurstVpinDirectional, HurstVpinDirectionalConfig,
};

let instrument_id = InstrumentId::from("PF_XBTUSD.KRAKEN");
let bar_type = BarType::from("PF_XBTUSD.KRAKEN-2000000-VALUE-LAST-INTERNAL");

let config = HurstVpinDirectionalConfig::new(
    instrument_id,
    bar_type,
    Quantity::from("0.01"),
);

engine.add_strategy(HurstVpinDirectional::new(config))?;
```

## Extending this strategy

This strategy is intentionally minimal. Common extensions:

- **Volatility gate**: overlay a realized-volatility estimator on the
  same bars to suppress entries during clearly chaotic sessions.
- **Alternative bars**: swap `VALUE` for `VALUE_IMBALANCE` or
  `VALUE_RUNS` to sample directly on information arrival.
- **Risk budgeting**: scale `trade_size` by current portfolio exposure
  rather than using a fixed quantity.
- **Multi-instrument**: run the strategy across a basket and allocate
  across signals by Hurst strength.
