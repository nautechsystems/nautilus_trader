# PositionClosed

`PositionClosed` represents a position having been closed. The `ExecutionEngine` emits it
when a fill flattens the position (see [From fill to position](index.md#from-fill-to-position-the-causal-chain)). Handler: `on_position_closed`.

## Fields

`PositionClosed` shares the position event field set. See [Position event fields](index.md#position-event-fields) for the full matrix across
the three events. The fields that distinguish `PositionClosed`:

| Field              | Python type     | Description                                            |
| ------------------ | --------------- | ------------------------------------------------------ |
| `closing_order_id` | `ClientOrderId` | The client order ID that closed the position.          |
| `avg_px_close`     | `float`         | The average close price.                               |
| `realized_return`  | `float`         | The realized return for the position.                  |
| `realized_pnl`     | `Money`         | The final realized PnL for the position.               |
| `duration_ns`      | `int`           | The total open duration (nanoseconds).                 |
| `ts_closed`        | `int`           | UNIX timestamp (nanoseconds) when the position closed. |

On close, `side` is `FLAT` and `unrealized_pnl` is zero.

## Example

Reading the event in a strategy handler:

```python
def on_position_closed(self, event: PositionClosed) -> None:
    self.log.info(
        f"Closed {event.instrument_id}: realized={event.realized_pnl} "
        f"return={event.realized_return}",
    )
```

## Related guides

- [Events](index.md) - Event categories, dispatch, and the fill-to-position chain.
- [Positions](../positions.md) - Position lifecycle, aggregation, and PnL.
- [Orders](../orders/) - Orders whose fills open and close positions.
