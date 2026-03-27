# TokenMM MakerV3 Shared Deque Rollout

This runbook covers the shared deque quote-stack rollout for the supported
TokenMM perp core:

- `plumeusdt_bybit_perp_makerv3`
- `plumeusdt_okx_perp_makerv3`
- `plumeusdt_bitget_perp_makerv3`

The goal is to keep deep ladders stable under venue rate limits while making
normal repricing visually deterministic:

- inward moves: place front, then cancel back
- outward moves: cancel front, then place back
- steady FV: no churn
- middle-of-stack changes: allowed only for true hole repair after fills or
  desync

The older bounded-convergence budgets still exist as safety rails, but the
live stack-management policy is now the shared deque planner rather than a
general middle-repricing convergence engine.

## Initial venue defaults

Roll out these explicit runtime defaults in strategy config:

- Bybit perp:
  - `max_cancels_per_side_per_cycle = 1`
  - `max_places_per_side_per_cycle = 2`
  - `max_total_actions_per_cycle = 4`
  - `max_pending_cancels_per_side = 1`
- OKX perp:
  - `max_cancels_per_side_per_cycle = 2`
  - `max_places_per_side_per_cycle = 2`
  - `max_total_actions_per_cycle = 6`
  - `max_pending_cancels_per_side = 2`
- Bitget perp:
  - `max_cancels_per_side_per_cycle = 1`
  - `max_places_per_side_per_cycle = 2`
  - `max_total_actions_per_cycle = 4`
  - `max_pending_cancels_per_side = 1`

These are rollout defaults, not a promise that deep ladders will fully settle in
one cycle. Healthy bounded convergence is allowed to lag the ideal ladder while
remaining safe and tradeable.

## Rollout order

1. Deploy the bounded-convergence build to all TokenMM nodes.
2. Restart the three perp nodes in bot-off mode.
3. Verify `Signal` / `Params` show runner truth and no startup reconciliation
   failure.
4. Enable OKX perp first.
5. Enable Bitget perp second.
6. Enable Bybit perp last.

Bybit goes last because it is the strictest venue on cancel/rate protection and
is the venue most likely to expose remaining churn bugs.

## What to watch

Primary signals:

- `quote_cycle` payload `backlog_mode`
- `decision_context_json.bounded_convergence`
- `decision_context_json.bounded_convergence.<side>.stack_action_mode`
- `decision_context_json.bounded_convergence.<side>.front_changed`
- `decision_context_json.bounded_convergence.<side>.back_changed`
- `decision_context_json.bounded_convergence.<side>.missing_level_count`
- `decision_context_json.bounded_convergence.<side>.interior_hole_count`
- persisted `order_action.reason_code`
- persisted `order_action.level_index`
- persisted `order_action.quote_cycle_id`
- `managed_orders` and per-side open depth
- venue-protection alerts
- cancel reject reasons by venue

Healthy steady-state behavior:

- `backlog_mode = normal` almost all the time
- `bounded_convergence.*.budget_limited = true` can be normal on deep ladders
- `bounded_convergence.*.backlog_limited = false` in steady state
- `stack_action_mode` is usually `no_op` and, when rebalanced, is dominated by
  `place_front_cancel_back`, `cancel_front_place_back`, or `place_missing`
- `front_changed` and `back_changed` match the expected deque transition for the
  side that moved
- `planned_cancel_count` and `executed_cancel_count` stay small and bounded
- the ladder changes from the edge and tail, not by broad side replacement

Degraded but acceptable transient behavior:

- brief `soft_throttle` after a burst of cancels
- temporary `total_missing_level_count > 0` while the ladder converges
- `planned_place_count > executed_place_count` due cycle budgets

Unhealthy behavior:

- repeated `hard_freeze` or any sustained `blocked`
- venue protection alerts
- repeated `order not exists`, `unknown order sent`, or rate-limit reject bursts
- managed order count drifting upward without matching venue depth

## Rollback

Immediate safety rollback:

- set `bot_on = false`

Throttle rollback when the strategy is alive but venue pressure is too high:

- set `max_cancels_per_side_per_cycle = 0`
- set `max_places_per_side_per_cycle = 1`
- set `max_total_actions_per_cycle = 1`

In this emergency throttle mode, bounded convergence alternates the first side
each quote cycle so the ladder does not refill one side only.

Deployment rollback:

- redeploy the previous release
- restart the perp node in bot-off mode
- do not re-enable until runner truth, backlog mode, and venue alerts are clean

## Healthy deep-ladder interpretation

Bounded convergence intentionally allows the live ladder to be "good enough"
before it is fully ideal. For deep books, treat these as healthy:

- outer levels filling in over multiple quote cycles
- passive tail levels persisting while the touch improves gradually
- occasional `budget_limited = true` without blocked state

Treat these as bugs or rollout blockers:

- whole-side cancel bursts on ordinary moves
- repeated `repair_hole` or `place_missing_hole_repair` on a seemingly stable
  book without corresponding fills/desync
- `cancel_front_violation` / `cancel_back_excess` rows that imply middle-stack
  mutation when joined back to the same `quote_cycle_id`
- repeated cancel-reject cleanup on already-gone orders
- loss of terminal blocked state in API/Signal surfaces

## SQLite audit queries

Use these checks against the live telemetry DBs when validating the rollout.

Recent quote-cycle deque transitions:

```sql
SELECT
  quote_cycle_id,
  reason_code,
  json_extract(decision_context_json, '$.bounded_convergence.buy.stack_action_mode') AS buy_mode,
  json_extract(decision_context_json, '$.bounded_convergence.buy.front_changed') AS buy_front_changed,
  json_extract(decision_context_json, '$.bounded_convergence.buy.back_changed') AS buy_back_changed,
  json_extract(decision_context_json, '$.bounded_convergence.sell.stack_action_mode') AS sell_mode,
  json_extract(decision_context_json, '$.bounded_convergence.sell.front_changed') AS sell_front_changed,
  json_extract(decision_context_json, '$.bounded_convergence.sell.back_changed') AS sell_back_changed
FROM quote_cycle
WHERE reason_code = 'completed_rebalanced'
ORDER BY ts_cycle_end_ns DESC
LIMIT 50;
```

Recent order-action deque intents:

```sql
SELECT
  quote_cycle_id,
  reason_code,
  level_index,
  client_order_id,
  order_status,
  ts_decision_ns
FROM order_action
WHERE reason_code IN (
  'cancel_front_violation',
  'cancel_back_excess',
  'place_front_improve',
  'place_back_backfill',
  'place_missing_hole_repair'
)
ORDER BY ts_decision_ns DESC
LIMIT 100;
```
