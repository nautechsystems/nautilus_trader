# OrderAccepted

`OrderAccepted` represents an order having been accepted by the trading venue. The
`ExecutionEngine` applies it to the order, updates the `Cache`, and publishes it on the
`MessageBus`. It fires when the venue acknowledges the order as received and valid (often
a FIX `NEW` OrdStatus).

Typical transition: `SUBMITTED` -> `ACCEPTED`. Handler: `on_order_accepted`.

## Fields

Beyond the [common Python order event fields](index.md#common-python-order-event-fields),
`OrderAccepted` carries:

| Field            | Python type    | Required/default | Description                            |
| ---------------- | -------------- | ---------------- | -------------------------------------- |
| `venue_order_id` | `VenueOrderId` | Required         | The venue‑assigned order identifier.   |
| `account_id`     | `AccountId`    | Required         | The account associated with the order. |
| `reconciliation` | `bool`         | Required         | If generated during reconciliation.    |

## Example

Reading the event in a strategy handler:

```python
def on_order_accepted(self, event: OrderAccepted) -> None:
    self.log.info(
        f"Order {event.client_order_id} accepted as {event.venue_order_id}",
    )
```

## Related guides

- [Events](index.md) - Event categories, dispatch, and the common order event fields.
- [Orders](../orders/) - Order types and the state machine.
