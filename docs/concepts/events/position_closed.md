# PositionClosed

`PositionClosed` records the final snapshot of a position. The `ExecutionEngine` emits it when a fill
flattens the position or a fill correction leaves the corrected position closed. See
[From fill to position](index.md#from-fill-to-position-the-causal-chain). Handler:
`on_position_closed`.

## Fields

See [Position event fields](index.md#position-event-fields) for the complete field matrix. The fields
that describe the close and final result are:

| Field              | Python type               | Description                                                 |
| ------------------ | ------------------------- | ----------------------------------------------------------- |
| `closing_order_id` | `ClientOrderId` or `None` | The client order ID that closed the position, if available. |
| `peak_quantity`    | `Quantity`                | The largest directional quantity reached by the position.   |
| `peak_qty`         | `Quantity`                | Compatibility alias for `peak_quantity`.                    |
| `avg_px_close`     | `float` or `None`         | The average close price, if available.                      |
| `realized_return`  | `float`                   | The final realized return for the position.                 |
| `realized_pnl`     | `Money` or `None`         | The final realized PnL, if available.                       |
| `unrealized_pnl`   | `Money`                   | Set to zero by the engine.                                  |
| `duration`         | `int`                     | The total open duration in nanoseconds.                     |
| `ts_opened`        | `int`                     | UNIX timestamp (nanoseconds) when the position opened.      |
| `ts_closed`        | `int` or `None`           | UNIX timestamp (nanoseconds) when the position closed.      |

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

- [Events](index.md) - Event categories, dispatch, and the fill‑to‑position chain.
- [Positions](../positions.md) - Position lifecycle, aggregation, and PnL.
- [Orders](../orders/) - Orders whose fills open and close positions.
