# OrderFillVoided

`OrderFillVoided` records that all or part of a previously reported fill no longer has economic
effect. The `ExecutionEngine` applies the correction to the order and positions, then refreshes
portfolio position and PnL caches before publishing it on the `MessageBus`. Venue adapters refresh
account balances from their authoritative account endpoints.

The correction updates cached position aggregates in place. It does not synthesize
`PositionChanged` or `PositionClosed`; strategies receive `OrderFillVoided` after the corrected
cache state is available.

A correction is not an opposite-side fill. It retains the original trade identity so replay,
reconciliation, and strategy audit history describe the venue action directly.

Handler: `on_order_fill_voided`.

## Quantity and status behavior

`voided_qty` and `commission_voided` are cumulative for the referenced `trade_id`. Quantity and fee
corrections cannot decrease. A later revision may increase either value or change `is_reopened` at
the same quantity. Duplicate, stale, and over-void corrections are rejected.

By default, a correction does not make the corrected quantity executable:

- A filled order becomes terminal `VOIDED`, even when some effective filled quantity survives.
- A partially filled order preserves the remainder that was already working. Its status derives
  from the surviving effective fills and its leaves exclude the non-reopened void quantity.
- A canceled or expired order keeps its terminal status.
- A correction with `is_reopened=true` also returns the corrected quantity to working leaves. The
  order derives `ACCEPTED` when no effective fill remains or `PARTIALLY_FILLED` when some quantity
  survives.

If Nautilus never applied the referenced fill, the event records the authoritative order-level
correction but does not reverse position or account exposure.

:::note
The schemas append this event and status without changing existing records. Older v2 readers do not
recognize the new values, so upgrade consumers before they read corrected streams or catalog data.
:::

## Fields

Beyond the [common order event fields](index.md#common-order-event-fields), `OrderFillVoided`
carries:

| Field               | Python type            | Required/default | Description                                             |
|---------------------|------------------------|------------------|---------------------------------------------------------|
| `correction_id`     | `str`                  | Required         | Identity for this correction revision.                  |
| `trade_id`          | `TradeId`              | Required         | Original venue trade ID.                                |
| `voided_qty`        | `Quantity`             | Required         | Cumulative ineffective quantity for the trade.          |
| `commission_voided` | `Money` or `None`      | `None`           | Cumulative fee correction for the trade.                 |
| `order_side`        | `OrderSide`            | Required         | Side of the original fill.                              |
| `order_type`        | `OrderType`            | Required         | Type of the original order.                             |
| `last_px`           | `Price`                | Required         | Price of the original fill.                             |
| `currency`          | `Currency`             | Required         | Currency of the original fill price.                    |
| `liquidity_side`    | `LiquiditySide`        | Required         | Liquidity side of the original fill.                    |
| `position_id`       | `PositionId` or `None` | `None`           | Position ID associated with the original fill.          |
| `reason`            | `str` or `None`        | `None`           | Venue or reconciliation reason for the correction.      |
| `info`              | `dict[str, str]` or `None` | `None`        | Additional venue correction metadata.                   |
| `is_reopened`       | `bool`                 | `False`          | Whether the venue proves the order is executable again. |

## Example

```python
def on_order_fill_voided(self, event: OrderFillVoided) -> None:
    self.log.warning(
        f"Corrected {event.trade_id}: voided={event.voided_qty} "
        f"reopened={event.is_reopened}",
    )
```

## Related guides

- [Execution](../execution.md) - Correction application and publication order.
- [OrderFilled](order_filled.md) - The original fill event.
- [Orders](../orders/) - Order status and state flow.
