# OrderCanceled

`OrderCanceled` represents an order having been canceled at the trading venue. The
`ExecutionEngine` applies it to the order, updates the `Cache`, and publishes it on the
`MessageBus`. It fires when the venue confirms cancellation of the order.

Transition: `PENDING_CANCEL` / `ACCEPTED` -> `CANCELED`. Handler: `on_order_canceled`.

## Fields

`OrderCanceled` carries only the [common order event fields](index.md#common-order-event-fields). On this event, `venue_order_id` and `account_id` are
usually populated but may be `None`, and `reconciliation` carries a real value.

## Example

Reading the event in a strategy handler:

```python
def on_order_canceled(self, event: OrderCanceled) -> None:
    self.log.info(f"Order {event.client_order_id} canceled")
```

## Related guides

- [Events](index.md) - Event categories, dispatch, and the common order event fields.
- [Orders](../orders/) - Order types and the state machine.
