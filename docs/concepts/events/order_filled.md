# OrderFilled

`OrderFilled` records a partial or full execution against an order. The `ExecutionEngine` applies it
to the order, updates the `Cache`, and publishes it on the `MessageBus`. Fills from live execution,
reconciliation, and simulated matching drive the position lifecycle events.

Typical transition: `ACCEPTED` -> `FILLED` / `PARTIALLY_FILLED`. Handler: `on_order_filled`.

## Fields

Beyond the [common Python order event fields](index.md#common-python-order-event-fields),
`OrderFilled` carries:

| Field            | Python type                | Required/default | Description                                                              |
| ---------------- | -------------------------- | ---------------- | ------------------------------------------------------------------------ |
| `venue_order_id` | `VenueOrderId`             | Required         | The venue-assigned order identifier.                                     |
| `account_id`     | `AccountId`                | Required         | The account associated with the fill.                                    |
| `trade_id`       | `TradeId`                  | Required         | The trade match ID assigned by the venue.                                |
| `position_id`    | `PositionId` or `None`     | `None`           | The position ID associated with the fill.                                |
| `order_side`     | `OrderSide`                | Required         | The execution order side.                                                |
| `order_type`     | `OrderType`                | Required         | The execution order type.                                                |
| `last_qty`       | `Quantity`                 | Required         | The fill quantity for this execution.                                    |
| `last_px`        | `Price`                    | Required         | The fill price for this execution, not the average price.                |
| `currency`       | `Currency`                 | Required         | The currency of the fill price.                                          |
| `commission`     | `Money` or `None`          | `None`           | The fill commission, if reported.                                        |
| `liquidity_side` | `LiquiditySide`            | Required         | The execution liquidity side (`MAKER`, `TAKER`, or `NO_LIQUIDITY_SIDE`). |
| `info`           | `dict[str, str]` or `None` | `None`           | Additional venue-specific or adapter-specific fill metadata.             |
| `reconciliation` | `bool`                     | Required         | If the event was generated during reconciliation.                        |

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
