# OrderTriggered

`OrderTriggered` represents an order having been triggered at the trading venue. The
`ExecutionEngine` applies it to the order, updates the `Cache`, and publishes it on the
`MessageBus`. It fires when a trigger condition is met for a limit-style conditional order
(`StopLimit`, `LimitIfTouched`, or `TrailingStopLimit`).

Transition: `ACCEPTED` -> `TRIGGERED`. Handler: `on_order_triggered`.

## Fields

`OrderTriggered` carries only the [common order event fields](index.md#common-order-event-fields). On this event, `venue_order_id` and `account_id`
are usually populated but may be `None`, and `reconciliation` carries a real value.

## Example

Reading the event in a strategy handler:

```python
def on_order_triggered(self, event: OrderTriggered) -> None:
    self.log.info(f"Order {event.client_order_id} triggered")
```

## Related guides

- [Events](index.md) - Event categories, dispatch, and the common order event fields.
- [Orders](../orders/) - Order types and the state machine.
