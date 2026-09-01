# OrderModifyRejected

`OrderModifyRejected` records that a `ModifyOrder` command was rejected. The `ExecutionEngine`
applies it to the order, updates the `Cache`, and publishes it on the `MessageBus`. A trading venue,
simulated matching engine, or local risk control can reject the request.

Typical transition: `PENDING_UPDATE` -> previous status (for example `ACCEPTED`). Handler:
`on_order_modify_rejected`.

## Fields

Beyond the [common Python order event fields](index.md#common-python-order-event-fields),
`OrderModifyRejected` carries:

| Field            | Python type              | Required/default | Description                                      |
| ---------------- | ------------------------ | ---------------- | ------------------------------------------------ |
| `reason`         | `str`                    | Required         | The order update rejection reason.               |
| `venue_order_id` | `VenueOrderId` or `None` | `None`           | The venue-assigned order identifier, if known.   |
| `account_id`     | `AccountId` or `None`    | `None`           | The account associated with the order, if known. |
| `reconciliation` | `bool`                   | Required         | If generated during reconciliation.              |

## Example

Reading the event in a strategy handler:

```python
def on_order_modify_rejected(self, event: OrderModifyRejected) -> None:
    self.log.warning(
        f"Modify rejected for {event.client_order_id}: {event.reason}",
    )
```

## Related guides

- [Events](index.md) - Event categories, dispatch, and the common order event fields.
- [Orders](../orders/) - Order types and the state machine.
