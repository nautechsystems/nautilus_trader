# AccountState

`AccountState` carries a snapshot of an account's balances and margins. The system publishes it when
the venue reports an account update through the execution client, or when the `Portfolio`
recalculates account state after a position update (for margin accounts with
`calculate_account_state` enabled). The `Portfolio` subscribes to these events internally
to maintain exposure and balance tracking.

The `is_reported` flag distinguishes venue‑reported snapshots from system‑calculated ones.

## Fields

| Field           | Python type            | Required/default | Description                                                               |
| --------------- | ---------------------- | ---------------- | ------------------------------------------------------------------------- |
| `account_id`    | `AccountId`            | Required         | The account ID (with the venue).                                          |
| `account_type`  | `AccountType`          | Required         | The account type (`CASH`, `MARGIN`, `BETTING`, or `WALLET`).              |
| `base_currency` | `Currency` or `None`   | `None`           | The account base currency (`None` for multi‑currency accounts).           |
| `is_reported`   | `bool`                 | Required         | If the state is reported from the exchange (otherwise system‑calculated). |
| `balances`      | `list[AccountBalance]` | Required         | The account balances (may be empty).                                      |
| `margins`       | `list[MarginBalance]`  | Required         | The margin balances (may be empty).                                       |
| `event_id`      | `UUID4`                | Required         | The event ID.                                                             |
| `ts_event`      | `int`                  | Required         | UNIX timestamp (nanoseconds) when the event occurred.                     |
| `ts_init`       | `int`                  | Required         | UNIX timestamp (nanoseconds) when the object was initialized.             |

## Example

Account state is normally consumed through the `Portfolio` rather than a dedicated handler:

```python
from nautilus_trader.model import Venue

# Account state is tracked by the portfolio; query it by venue
account = self.portfolio.account(venue=Venue("BINANCE"))
self.log.info(f"Account state: {account}")
```

The result is detached from the Portfolio. Mutating the returned account does not change the
authoritative account held by the engine.

## Related guides

- [Events](index.md) - Event categories and dispatch.
- [Accounting](../accounting.md) - Account types, balances, and margin models.
- [Portfolio](../portfolio.md) - How account state feeds exposure and balance tracking.
