# OrderCanceled

`OrderCanceled` records a confirmed cancellation. The execution pipeline applies it to the order,
updates the `Cache`, and publishes it on the `MessageBus`. It can come from a trading venue,
simulated matching engine, local order emulator, or reconciliation.

Typical transition: `PENDING_CANCEL` / `ACCEPTED` -> `CANCELED`. Handler: `on_order_canceled`.

## Fields

Beyond the [common Python order event fields](index.md#common-python-order-event-fields),
`OrderCanceled` carries:

| Field            | Python type              | Required/default | Description                                      |
| ---------------- | ------------------------ | ---------------- | ------------------------------------------------ |
| `venue_order_id` | `VenueOrderId` or `None` | `None`           | The venue-assigned order identifier, if known.   |
| `account_id`     | `AccountId` or `None`    | `None`           | The account associated with the order, if known. |
| `reconciliation` | `bool`                   | Required         | If generated during reconciliation.              |

## Example

Reading the event in a strategy handler:

```python
def on_order_canceled(self, event: OrderCanceled) -> None:
    self.log.info(f"Order {event.client_order_id} canceled")
```

## Related guides

- [Events](index.md) - Event categories, dispatch, and the common order event fields.
- [Orders](../orders/) - Order types and the state machine.
