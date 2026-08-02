# OrderAccepted

`OrderAccepted` represents an order having been accepted by the trading venue. The
`ExecutionEngine` applies it to the order, updates the `Cache`, and publishes it on the
`MessageBus`. It fires when the venue acknowledges the order as received and valid (often
a FIX `NEW` OrdStatus).

Transition: `SUBMITTED` -> `ACCEPTED`. Handler: `on_order_accepted`.

## Fields

`OrderAccepted` carries only the [common order event fields](index.md#common-order-event-fields). On this event, `venue_order_id` and `account_id` are
populated, and `reconciliation` carries a real value (default `False`).

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
