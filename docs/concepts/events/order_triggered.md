# OrderTriggered

`OrderTriggered` records that a limit‑style conditional order has triggered. The execution pipeline
applies it to the order, updates the `Cache`, and publishes it on the `MessageBus`. A trading venue,
simulated matching engine, or reconciliation can report the trigger for a `StopLimit`,
`LimitIfTouched`, or `TrailingStopLimit` order.

Typical transition: `ACCEPTED` -> `TRIGGERED`. Handler: `on_order_triggered`.

## Fields

Beyond the [common Python order event fields](index.md#common-python-order-event-fields),
`OrderTriggered` carries:

| Field            | Python type              | Required/default | Description                                      |
| ---------------- | ------------------------ | ---------------- | ------------------------------------------------ |
| `venue_order_id` | `VenueOrderId` or `None` | `None`           | The venue‑assigned order identifier, if known.   |
| `account_id`     | `AccountId` or `None`    | `None`           | The account associated with the order, if known. |
| `reconciliation` | `bool`                   | Required         | If generated during reconciliation.              |

## Example

Reading the event in a strategy handler:

```python
def on_order_triggered(self, event: OrderTriggered) -> None:
    self.log.info(f"Order {event.client_order_id} triggered")
```

## Related guides

- [Events](index.md) - Event categories, dispatch, and the common order event fields.
- [Orders](../orders/) - Order types and the state machine.
