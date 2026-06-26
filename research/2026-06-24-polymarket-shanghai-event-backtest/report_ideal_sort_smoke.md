# Ideal-sort smoke backtest results

Date: 2026-06-26
Scope: two curated Shanghai temperature Polymarket events from PMXT parquet, selected winning YES token only.

This note records the first "ideal replay" pass. It intentionally assumes the sorted historical sequence is the replay sequence, while leaving the stricter `timestamp` vs `timestamp_received` contract for later.

## 1. Replay ordering used in this pass

Current replay code sorts PMXT rows by:

```text
timestamp, timestamp_received, original_row
```

Meaning:

- `timestamp` is treated as the ideal source/event-time order.
- `timestamp_received` is only the tie-breaker and price_change batch boundary.
- `original_row` keeps sorting stable when both timestamps are equal.
- `price_change` rows with the same `(timestamp_received, timestamp, market, asset_id, event_type)` are applied as one batch before comparing PMXT batch-level BBO.

This is the right mode for a first "can we do useful research if the data is sorted into ideal order" test. It is not yet the final live-realistic replay contract.

## 2. Replay quality checks

| Event | Market | Price-change batch BBO mismatch | Snapshot BBO mismatch | Raw snapshot BBO mismatch | Trade off-book rate |
| --- | --- | ---: | ---: | ---: | ---: |
| highest-temperature-in-shanghai-on-june-9-2026 | 25C YES | 4.13% | 14.72% | 24.41% | 3.80% |
| highest-temperature-in-shanghai-on-june-10-2026 | 28C YES | 2.91% | 8.94% | 20.49% | 0.97% |

Interpretation:

- The replay is usable for an ideal-sort smoke test: event processing completes, L2 books are maintained, simple strategies can run end-to-end, and trade-vs-book checks are not obviously broken.
- It is not yet a validated production backtest: BBO mismatch is still non-zero, especially around snapshots. These mismatches should remain visible in every result artifact.
- The June 10 sample looks cleaner than June 9 by these diagnostics.

## 3. Simple strategy smoke results

All strategies below use the selected winning YES token. `settlement_pnl` assumes final YES payout = 1.

| Event | Market | Strategy | Fills | Ending inventory | Gross notional | Settlement PnL | Return on gross notional |
| --- | --- | --- | ---: | ---: | ---: | ---: | ---: |
| Jun 9 | 25C YES | maker_bbo | 287 | -71.78 | 1144.85 | -31.45 | -2.75% |
| Jun 9 | 25C YES | buy_hold_first_ask | 1 | 10.00 | 2.20 | 7.80 | 354.55% |
| Jun 9 | 25C YES | momentum_taker | 34 | 20.00 | 129.56 | -11.84 | -9.14% |
| Jun 9 | 25C YES | contrarian_taker | 34 | -20.00 | 126.59 | -3.15 | -2.49% |
| Jun 10 | 28C YES | maker_bbo | 182 | -52.59 | 767.51 | -53.87 | -7.02% |
| Jun 10 | 28C YES | buy_hold_first_ask | 1 | 10.00 | 2.30 | 7.70 | 334.78% |
| Jun 10 | 28C YES | momentum_taker | 27 | 90.00 | 145.70 | 26.50 | 18.19% |
| Jun 10 | 28C YES | contrarian_taker | 27 | -90.00 | 143.90 | -36.70 | -25.50% |

## 4. Sanity read

These results are broadly reasonable for a first pass:

1. `buy_hold_first_ask` is positive on both events because both selected YES tokens resolve YES. This is the simplest settlement sanity check.
2. June 10 momentum is positive and contrarian is negative, consistent with a winning YES token that had favorable upward movement in the replay path.
3. The `maker_bbo` result should not be interpreted as evidence that maker strategies are bad. The current fill model is intentionally naive/harsh and is mainly a plumbing test.
4. The strategy outputs are marked `smoke_test_unvalidated`; that label is correct and should stay until replay ordering, snapshot handling, fees, rewards, settlement, and fill model assumptions are hardened.

## 5. Current conclusion

For the near-term research loop, we can proceed with this ideal-sort mode:

```text
sort by source timestamp -> group same-message price_change batches -> replay L2 -> run simple strategies -> carry replay-quality diagnostics into every result
```

This is good enough to start organizing strategy experiments and factor-style research. It is not yet good enough to claim production-grade historical execution accuracy.

The next hardening step is to decide the official replay contract for PMXT historical data:

- source-time ideal replay,
- receive-time replay,
- or hybrid replay with snapshot resets and explicit drift markers.
