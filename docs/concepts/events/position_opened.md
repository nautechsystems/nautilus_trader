# PositionOpened

`PositionOpened` represents a position having been opened. The `ExecutionEngine` emits it
when a fill creates a new position (see [From fill to position](index.md#from-fill-to-position-the-causal-chain)). Handler: `on_position_opened`.

## Fields

`PositionOpened` shares the position event field set. See [Position event fields](index.md#position-event-fields) for the full matrix across
the three events. The fields that distinguish `PositionOpened`:

| Field          | Python type    | Description                                    |
| -------------- | -------------- | ---------------------------------------------- |
| `entry`        | `OrderSide`    | The entry order side that opened the position. |
| `side`         | `PositionSide` | The current position side (`LONG` or `SHORT`). |
| `quantity`     | `Quantity`     | The current open quantity.                     |
| `avg_px_open`  | `float`        | The average open price.                        |
| `realized_pnl` | `Money`        | The realized PnL for the position.             |

At open, `closing_order_id` is `None`, and `avg_px_close` and `realized_return` are zero.

## Example

Reading the event in a strategy handler:

```python
def on_position_opened(self, event: PositionOpened) -> None:
    self.log.info(
        f"Opened {event.side} {event.quantity} {event.instrument_id} "
        f"@ {event.avg_px_open}",
    )
```

## Related guides

- [Events](index.md) - Event categories, dispatch, and the fill-to-position chain.
- [Positions](../positions.md) - Position lifecycle, aggregation, and PnL.
- [Orders](../orders/) - Orders whose fills open and close positions.
