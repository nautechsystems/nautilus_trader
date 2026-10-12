# Execution Actions

## Purpose

An execution action is a strategy-initiated venue write which is not a submitted order. It has a
correlated delivery result, but it does not create a cached order, an order state machine, or a
synthetic settlement callback. Existing orderless accounting fills still carry internal fill and
leg identifiers. The first standard action family is a binary-option inventory conversion:

| Action                      | Economic effect                                       |
| --------------------------- | ----------------------------------------------------- |
| `BinaryOptionAction::Split` | Convert quote collateral `A` into `A` Yes and `A` No. |
| `BinaryOptionAction::Merge` | Convert `A` matched Yes/No into quote collateral.     |

The canonical command and result are `ExecuteAction` and `ExecutionActionResult`.

```text
Strategy
  -> ExecuteAction { BinaryOptionAction { pair, quantity, params } }
  -> ExecutionEngine (client/account routing only)
  -> execution client
  -> ExecutionActionResult { command_id, Accepted | Rejected | Ambiguous }

Venue account/fill/position state
  -> existing reconciliation, account, and position paths
```

`ExecutionAction` is a closed common type family. It currently contains only
`ExecutionAction::BinaryOption(BinaryOptionAction)`. New action families require a common
typed payload with defined semantics.

## Public strategy API

Rust accepts any currently supported standard action through the single generic method:

```rust
self.execute_action(
    self.account_id,
    Some(self.client_id),
    BinaryOptionAction::Split {
        pair: BinaryOptionPair::new(self.yes_id, self.no_id)?,
        quantity: Quantity::from("1.25"),
        params: None,
    },
)?;
```

The `From<BinaryOptionAction> for ExecutionAction` conversion happens at the strategy boundary.
Strategies do not know a venue action name or reconstruct a venue market identifier.

Python uses the equivalent immutable thin object. It has no routing or Hyperliquid-specific logic:

```python
action = BinaryOptionAction.split(
    yes_instrument_id=self.yes_id,
    no_instrument_id=self.no_id,
    quantity=Quantity.from_str("1.25"),
)
self.execute_action(
    account_id=self.account_id,
    action=action,
    client_id=self.client_id,
)
```

`quantity` is required and must be positive for both operations. There is no strategy-level
"merge maximum" shorthand: a concrete amount avoids silently choosing between account-wide and
strategy-owned inventory. `params` belongs to the typed action, not the command envelope. It is reserved for venue-specific options and every adapter must
either validate it completely or reject it; silently ignoring a supplied option is invalid.

`BinaryOptionPair` is the action's complete economic identity: canonical Yes instrument ID plus
canonical No instrument ID. The constructor rejects duplicate IDs. The common layer does not use a
cross-venue `OutcomeId`, a numeric `MarketId`, or a condition ID.

## Delivery result semantics

`ExecutionActionResult` is an ACK for the observed delivery outcome, not evidence that account or
inventory convergence has completed.

| Result      | Evidence                                                                                             | Strategy behavior                                     |
| ----------- | ---------------------------------------------------------------------------------------------------- | ----------------------------------------------------- |
| `Accepted`  | The active client received an explicit successful venue response.                                    | Await normal account/position/reconciliation updates. |
| `Rejected`  | The request was not sent, or the venue explicitly rejected it.                                       | Treat as not applied under available evidence.        |
| `Ambiguous` | The request may have reached the venue but the active client cannot establish a definitive response. | Do not blindly retry; reconcile ordinary venue state. |

A client reports `Rejected` when validation or routing prevents delivery, the operation is
unsupported, or the venue explicitly rejects it. After an attempted send, a timeout, disconnect,
or undecodable response can leave delivery `Ambiguous`. Adapters use the existing
`CommandFailure::{NotSent, VenueRejected, Ambiguous}` evidence to classify these outcomes.

If a client generation is stopped, disconnected, reset, or disposed, its cancelled task emits no
late result; after process failure no result can be delivered.

This follows the relevant `QueryAccount` / `QueryOrder` boundary without copying their lack of an
ACK: queries are observations, while an action is a venue write that needs a correlated delivery
notification.

## State reconciliation

Delivery results and state observations are separate. An accepted action does not itself emit an
order, fill, account, or position event. Venue fills and snapshots enter the existing reconciliation
and account paths, and may arrive before or after the delivery result.

Conversion fills can use the existing orderless leg-fill accounting path. These accounting events
retain the existing fill and position callbacks without creating a submitted order lifecycle.
External instrument claims determine strategy ownership; they do not correlate a venue fill with
an action command. Existing trade-ID deduplication and snapshot guards apply.

`FillReport.info` is optional provenance supplied by an adapter. Older serialized reports can omit
it. The model stores this metadata; execution reconciliation interprets it when recognizing
conversion fills.

For pair validation, WebSocket delivery, fill provenance, and account refresh behavior, see the
[Hyperliquid integration](../integrations/hyperliquid.md#advanced-workflows).

## Sandbox and backtest support

Sandbox and backtest execution clients currently reject execution actions without changing account
or position state. Simulation profiles, conversion fees, and simulated inventory changes are
reserved for a separate extension.
