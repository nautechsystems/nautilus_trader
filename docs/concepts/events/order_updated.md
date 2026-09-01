# OrderUpdated

`OrderUpdated` records a change to an order's quantity, price, or trigger price. The execution
pipeline applies it to the order, updates the `Cache`, and publishes it on the `MessageBus`. The
change can come from a trading venue, simulated matching engine, local order emulator, or
reconciliation.

Typical transition: `PENDING_UPDATE` -> previous status (for example `ACCEPTED`). Handler:
`on_order_updated`.

## Fields

Beyond the [common Python order event fields](index.md#common-python-order-event-fields),
`OrderUpdated` carries:

| Field               | Python type              | Required/default | Description                                                 |
| ------------------- | ------------------------ | ---------------- | ----------------------------------------------------------- |
| `venue_order_id`    | `VenueOrderId` or `None` | `None`           | The venue-assigned order identifier, if known.              |
| `account_id`        | `AccountId` or `None`    | `None`           | The account associated with the order, if known.            |
| `quantity`          | `Quantity`               | Required         | The order's current quantity.                               |
| `price`             | `Price` or `None`        | `None`           | The order's current price.                                  |
| `trigger_price`     | `Price` or `None`        | `None`           | The order's current trigger price.                          |
| `is_quote_quantity` | `bool`                   | `False`          | If the order quantity is denominated in the quote currency. |
| `reconciliation`    | `bool`                   | Required         | If generated during reconciliation.                         |

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
