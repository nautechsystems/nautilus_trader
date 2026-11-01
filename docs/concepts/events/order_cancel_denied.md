# OrderCancelDenied

`OrderCancelDenied` records a local refusal of a `CancelOrder` command that was not sent to its
execution target. The execution pipeline applies it to the order when cached and publishes it on the
`MessageBus`. Handler: `on_order_cancel_denied`.

The denial preserves the working order and its terms. See [Command refusals](index.md#command-refusals)
for pending-command resolution, causation, and callback migration.

## Fields

Beyond the [common Python order event fields](index.md#common-python-order-event-fields),
`OrderCancelDenied` carries:

| Field            | Python type              | Required/default | Description                                      |
| ---------------- | ------------------------ | ---------------- | ------------------------------------------------ |
| `reason`         | `str`                    | Required         | The reason the command was denied.               |
| `venue_order_id` | `VenueOrderId` or `None` | `None`           | The venue-assigned order identifier, if known.   |
| `account_id`     | `AccountId` or `None`    | `None`           | The account associated with the order, if known. |

## Example

Reading the event in a strategy handler:

```python
from nautilus_trader.model import OrderCancelDenied


def on_order_cancel_denied(self, event: OrderCancelDenied) -> None:
    self.log.warning(
        f"Cancel denied for {event.client_order_id}: {event.reason}",
    )
```

## Related guides

- [Events](index.md) - Event categories, dispatch, and the common order event fields.
- [Orders](../orders/) - Order types and the state machine.
