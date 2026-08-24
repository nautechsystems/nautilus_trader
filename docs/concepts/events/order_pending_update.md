# OrderPendingUpdate

`OrderPendingUpdate` represents a `ModifyOrder` command having been sent to the trading
venue. The `ExecutionEngine` applies it to the order, updates the `Cache`, and publishes
it on the `MessageBus`. It fires when the system dispatches a modify request and awaits
venue acknowledgement.

Typical transition: `ACCEPTED` -> `PENDING_UPDATE`. Handler: `on_order_pending_update`.

## Fields

Beyond the [common Python order event fields](index.md#common-python-order-event-fields),
`OrderPendingUpdate` carries:

| Field            | Python type              | Required/default | Description                                      |
| ---------------- | ------------------------ | ---------------- | ------------------------------------------------ |
| `venue_order_id` | `VenueOrderId` or `None` | `None`           | The venue-assigned order identifier, if known.   |
| `account_id`     | `AccountId` or `None`    | Required         | The account associated with the order, if known. |
| `reconciliation` | `bool`                   | Required         | If generated during reconciliation.              |

## Example

Reading the event in a strategy handler:

```python
def on_order_pending_update(self, event: OrderPendingUpdate) -> None:
    self.log.info(f"Modify pending for {event.client_order_id}")
```

## Related guides

- [Events](index.md) - Event categories, dispatch, and the common order event fields.
- [Orders](../orders/) - Order types and the state machine.
