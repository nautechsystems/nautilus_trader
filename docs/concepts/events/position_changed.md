# PositionChanged

`PositionChanged` represents a position having changed. The `ExecutionEngine` emits it
when a subsequent fill changes an open position's quantity or side (see [From fill to position](index.md#from-fill-to-position-the-causal-chain)). Handler:
`on_position_changed`.

## Fields

`PositionChanged` shares the position event field set. See [Position event fields](index.md#position-event-fields) for the full matrix across
the three events. The fields that distinguish `PositionChanged`:

| Field             | Python type | Description                                            |
| ----------------- | ----------- | ------------------------------------------------------ |
| `peak_qty`        | `Quantity`  | The peak directional quantity reached by the position. |
| `avg_px_close`    | `float`     | The average close price so far.                        |
| `realized_return` | `float`     | The realized return for the position.                  |
| `realized_pnl`    | `Money`     | The realized PnL for the position.                     |
| `unrealized_pnl`  | `Money`     | The unrealized PnL for the position.                   |

While the position remains open, `closing_order_id` is still `None`.

## Example

Reading the event in a strategy handler:

```python
def on_position_changed(self, event: PositionChanged) -> None:
    self.log.info(
        f"Changed {event.instrument_id} to {event.signed_qty} "
        f"(realized={event.realized_pnl})",
    )
```

## Related guides

- [Events](index.md) - Event categories, dispatch, and the fill-to-position chain.
- [Positions](../positions.md) - Position lifecycle, aggregation, and PnL.
- [Orders](../orders/) - Orders whose fills open and close positions.
