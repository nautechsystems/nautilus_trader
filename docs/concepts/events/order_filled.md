# OrderFilled

`OrderFilled` represents an order having been filled at the exchange. The
`ExecutionEngine` applies it to the order, updates the `Cache`, and publishes it on the
`MessageBus`. It fires when the venue reports a partial or full execution against the
order, and in turn drives the position events.

Transition: `ACCEPTED` -> `FILLED` / `PARTIALLY_FILLED`. Handler: `on_order_filled`.

## Fields

Beyond the [common order event fields](index.md#common-order-event-fields), `OrderFilled` carries:

| Field            | Python type            | Required/default | Description                                                              |
| ---------------- | ---------------------- | ---------------- | ------------------------------------------------------------------------ |
| `trade_id`       | `TradeId`              | Required         | The trade match ID (assigned by the venue).                              |
| `position_id`    | `PositionId` or `None` | Required         | The position ID associated with the fill (assigned by the venue).        |
| `order_side`     | `OrderSide`            | Required         | The execution order side.                                                |
| `order_type`     | `OrderType`            | Required         | The execution order type.                                                |
| `last_qty`       | `Quantity`             | Required         | The fill quantity for this execution.                                    |
| `last_px`        | `Price`                | Required         | The fill price for this execution (not the average price).               |
| `currency`       | `Currency`             | Required         | The currency of the fill price.                                          |
| `commission`     | `Money`                | Required         | The fill commission.                                                     |
| `liquidity_side` | `LiquiditySide`        | Required         | The execution liquidity side (`MAKER`, `TAKER`, or `NO_LIQUIDITY_SIDE`). |
| `info`           | `dict[str, object]`    | `None`           | Additional fill information (coerced to `{}` when omitted).              |

On this event, `venue_order_id` and `account_id` are populated, and `reconciliation`
carries a real value.

## Example

Reading the event in a strategy handler:

```python
def on_order_filled(self, event: OrderFilled) -> None:
    self.log.info(
        f"Filled {event.last_qty} @ {event.last_px} "
        f"({event.liquidity_side}) commission={event.commission}",
    )
```

## Related guides

- [Events](index.md) - Event categories, dispatch, and the common order event fields.
- [Positions](../positions.md) - Positions created and modified from fills.
- [Orders](../orders/) - Order types and the state machine.
