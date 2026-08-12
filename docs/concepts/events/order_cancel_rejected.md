# OrderCancelRejected

`OrderCancelRejected` represents a `CancelOrder` command having been rejected by the
trading venue. The `ExecutionEngine` applies it to the order, updates the `Cache`, and
publishes it on the `MessageBus`. It fires when the venue rejects a cancel request.

Typical transition: `PENDING_CANCEL` -> previous status (for example `ACCEPTED`). Handler:
`on_order_cancel_rejected`.

## Fields

Beyond the [common Python order event fields](index.md#common-python-order-event-fields),
`OrderCancelRejected` carries:

| Field            | Python type              | Required/default | Description                                      |
| ---------------- | ------------------------ | ---------------- | ------------------------------------------------ |
| `reason`         | `str`                    | Required         | The order cancel rejection reason.               |
| `venue_order_id` | `VenueOrderId` or `None` | `None`           | The venue‑assigned order identifier, if known.   |
| `account_id`     | `AccountId` or `None`    | `None`           | The account associated with the order, if known. |
| `reconciliation` | `bool`                   | Required         | If generated during reconciliation.              |

## Example

Reading the event in a strategy handler:

```python
def on_order_cancel_rejected(self, event: OrderCancelRejected) -> None:
    self.log.warning(
        f"Cancel rejected for {event.client_order_id}: {event.reason}",
    )
```

## Related guides

- [Events](index.md) - Event categories, dispatch, and the common order event fields.
- [Orders](../orders/) - Order types and the state machine.
