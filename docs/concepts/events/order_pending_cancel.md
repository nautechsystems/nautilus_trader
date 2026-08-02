# OrderPendingCancel

`OrderPendingCancel` represents a `CancelOrder` command having been sent to the trading
venue. The `ExecutionEngine` applies it to the order, updates the `Cache`, and publishes
it on the `MessageBus`. It fires when the system dispatches a cancel request and awaits
venue acknowledgement.

Transition: `ACCEPTED` -> `PENDING_CANCEL`. Handler: `on_order_pending_cancel`.

## Fields

`OrderPendingCancel` carries only the [common order event fields](index.md#common-order-event-fields). On this event, `venue_order_id` and
`account_id` are usually populated but may be `None`, and `reconciliation` carries a real
value.

## Example

Reading the event in a strategy handler:

```python
def on_order_pending_cancel(self, event: OrderPendingCancel) -> None:
    self.log.info(f"Cancel pending for {event.client_order_id}")
```

## Related guides

- [Events](index.md) - Event categories, dispatch, and the common order event fields.
- [Orders](../orders/) - Order types and the state machine.
