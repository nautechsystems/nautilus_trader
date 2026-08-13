# PositionOpened

`PositionOpened` records the opening snapshot of a new position. The `ExecutionEngine` emits it when
a fill creates the position (see
[From fill to position](index.md#from-fill-to-position-the-causal-chain)). Handler:
`on_position_opened`.

## Fields

See [Position event fields](index.md#position-event-fields) for the complete field matrix.
`PositionOpened` contains the opening snapshot; later events expose additional aggregate and
lifecycle fields. At opening, realized PnL subtracts the opening fill's commission when that
commission is denominated in the instrument's
[cost currency](../positions.md#currency-considerations). Otherwise, it is zero. A reopened
`NETTING` position starts a new cycle; realized PnL from the prior cycle remains in the
[closed position snapshot](../positions.md#position-snapshotting).

Its main state fields are:

| Field          | Python type       | Description                                        |
| -------------- | ----------------- | -------------------------------------------------- |
| `entry`        | `OrderSide`       | The entry order side that opened the position.     |
| `side`         | `PositionSide`    | The current position side (`LONG` or `SHORT`).     |
| `signed_qty`   | `float`           | The signed position quantity.                      |
| `quantity`     | `Quantity`        | The current open quantity.                         |
| `last_qty`     | `Quantity`        | The quantity of the fill that opened the position. |
| `last_px`      | `Price`           | The price of the fill that opened the position.    |
| `currency`     | `Currency`        | The position quote currency.                       |
| `avg_px_open`  | `float`           | The average open price.                            |
| `realized_pnl` | `Money` or `None` | The current cycle's realized PnL in cost currency. |

## Example

Reading the event in a strategy handler:

```python
def on_position_opened(self, event: PositionOpened) -> None:
    self.log.info(
        f"Opened {event.side} {event.quantity} {event.instrument_id} @ {event.avg_px_open}",
    )
```

## Related guides

- [Events](index.md) - Event categories, dispatch, and the fill‑to‑position chain.
- [Positions](../positions.md) - Position lifecycle, aggregation, and PnL.
- [Orders](../orders/) - Orders whose fills open and close positions.
