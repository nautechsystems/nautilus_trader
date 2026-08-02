# OrderUpdated

`OrderUpdated` represents an order having been updated at the trading venue. The
`ExecutionEngine` applies it to the order, updates the `Cache`, and publishes it on the
`MessageBus`. It fires when the venue confirms a modification to quantity, price, or
trigger price.

Transition: `PENDING_UPDATE` -> previous status (for example `ACCEPTED`). Handler:
`on_order_updated`.

## Fields

Beyond the [common order event fields](index.md#common-order-event-fields), `OrderUpdated` carries:

| Field               | Python type       | Required/default | Description                                                 |
| ------------------- | ----------------- | ---------------- | ----------------------------------------------------------- |
| `quantity`          | `Quantity`        | Required         | The order's current quantity.                               |
| `price`             | `Price` or `None` | Required         | The order's current price.                                  |
| `trigger_price`     | `Price` or `None` | Required         | The order's current trigger price.                          |
| `is_quote_quantity` | `bool`            | `False`          | If the order quantity is denominated in the quote currency. |

On this event, `venue_order_id` and `account_id` are usually populated but may be `None`,
and `reconciliation` carries a real value.

## Example

Reading the event in a strategy handler:

```python
def on_order_updated(self, event: OrderUpdated) -> None:
    self.log.info(
        f"Order {event.client_order_id} updated: qty={event.quantity} price={event.price}",
    )
```

## Related guides

- [Events](index.md) - Event categories, dispatch, and the common order event fields.
- [Orders](../orders/) - Order types and the state machine.
