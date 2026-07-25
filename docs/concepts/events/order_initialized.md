# OrderInitialized

`OrderInitialized` represents an order having been initialized. The `ExecutionEngine`
applies it to the order, updates the `Cache`, and publishes it on the `MessageBus`. It is
the seed event that carries enough information to send an order over the wire and
reconstruct it identically.

Created locally as the seed event for a new order. Handler: `on_order_initialized`.

## Fields

Beyond the [common order event fields](index.md#common-order-event-fields), `OrderInitialized` carries:

| Field                   | Python type                     | Required/default | Description                                                        |
| ----------------------- | ------------------------------- | ---------------- | ------------------------------------------------------------------ |
| `side`                  | `OrderSide`                     | Required         | The order side (exposed as `event.side`).                          |
| `order_type`            | `OrderType`                     | Required         | The order type.                                                    |
| `quantity`              | `Quantity`                      | Required         | The order quantity.                                                |
| `time_in_force`         | `TimeInForce`                   | Required         | The order time in force.                                           |
| `post_only`             | `bool`                          | Required         | If the order will only provide liquidity (make a market).          |
| `reduce_only`           | `bool`                          | Required         | If the order carries the 'reduce‑only' execution instruction.      |
| `quote_quantity`        | `bool`                          | Required         | If the order quantity is denominated in the quote currency.        |
| `options`               | `dict[str, str]`                | Required         | Order initialization options for specific order parameters.        |
| `emulation_trigger`     | `TriggerType`                   | `NO_TRIGGER`     | The market price trigger for local order emulation.                |
| `trigger_instrument_id` | `InstrumentId` or `None`        | Required         | The emulation trigger instrument ID (defaults to `instrument_id`). |
| `contingency_type`      | `ContingencyType`               | Required         | The order contingency type.                                        |
| `order_list_id`         | `OrderListId` or `None`         | Required         | The order list ID associated with the order.                       |
| `linked_order_ids`      | `list[ClientOrderId]` or `None` | Required         | The linked client order ID(s).                                     |
| `parent_order_id`       | `ClientOrderId` or `None`       | Required         | The order's parent client order ID.                                |
| `exec_algorithm_id`     | `ExecAlgorithmId` or `None`     | Required         | The execution algorithm ID for the order.                          |
| `exec_algorithm_params` | `dict[str, Any]` or `None`      | Required         | The execution algorithm parameters.                                |
| `exec_spawn_id`         | `ClientOrderId` or `None`       | Required         | The execution algorithm spawning primary client order ID.          |
| `tags`                  | `list[str]` or `None`           | Required         | The custom user tags for the order.                                |

On this event, `venue_order_id` and `account_id` are both `None`, and `ts_event` equals
`ts_init`. The `reconciliation` property always returns `False` here, even for orders
reconstructed during reconciliation; later order events such as [`OrderAccepted`](order_accepted.md) carry the real value.

## Example

Reading the event in a strategy handler:

```python
def on_order_initialized(self, event: OrderInitialized) -> None:
    self.log.info(
        f"Initialized {event.order_type} {event.side} "
        f"{event.quantity} {event.instrument_id}",
    )
```

## Related guides

- [Events](index.md) - Event categories, dispatch, and the common order event fields.
- [Orders](../orders/) - Order types and the state machine.
