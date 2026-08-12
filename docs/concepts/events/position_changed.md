# PositionChanged

`PositionChanged` records an update that leaves a position open. The `ExecutionEngine` emits it when
a fill changes an open position without closing it, or when a fill correction leaves the corrected
position open. See [From fill to position](index.md#from-fill-to-position-the-causal-chain).
Handler: `on_position_changed`.

## Fields

See [Position event fields](index.md#position-event-fields) for the complete field matrix. In addition
to the opening snapshot fields, `PositionChanged` exposes:

| Field             | Python type       | Description                                                  |
| ----------------- | ----------------- | ------------------------------------------------------------ |
| `peak_quantity`   | `Quantity`        | The largest directional quantity reached by the position.    |
| `peak_qty`        | `Quantity`        | Compatibility alias for `peak_quantity`.                     |
| `avg_px_close`    | `float` or `None` | The average close price so far, if any quantity has closed.  |
| `realized_return` | `float`           | The realized return for the position.                        |
| `realized_pnl`    | `Money` or `None` | The realized PnL, if available.                              |
| `unrealized_pnl`  | `Money`           | Set to zero by the engine, not a mark‑to‑market calculation. |
| `ts_opened`       | `int`             | UNIX timestamp (nanoseconds) when the position opened.       |

## Example

Reading the event in a strategy handler:

```python
def on_position_changed(self, event: PositionChanged) -> None:
    self.log.info(
        f"Changed {event.instrument_id} to {event.signed_qty} (realized={event.realized_pnl})",
    )
```

## Related guides

- [Events](index.md) - Event categories, dispatch, and the fill‑to‑position chain.
- [Positions](../positions.md) - Position lifecycle, aggregation, and PnL.
- [Orders](../orders/) - Orders whose fills open and close positions.
