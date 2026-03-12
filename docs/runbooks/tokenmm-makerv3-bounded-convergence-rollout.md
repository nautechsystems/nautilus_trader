# TokenMM MakerV3 Bounded-Convergence Rollout

This runbook covers the bounded-convergence rollout for the supported TokenMM
perp core:

- `plumeusdt_bybit_perp_makerv3`
- `plumeusdt_okx_perp_makerv3`
- `plumeusdt_bitget_perp_makerv3`

The goal is to keep deep ladders stable under venue rate limits by converging
incrementally instead of refreshing whole sides in one quote cycle.

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
- `managed_orders` and per-side open depth
- venue-protection alerts
- cancel reject reasons by venue

Healthy steady-state behavior:

- `backlog_mode = normal` almost all the time
- `bounded_convergence.*.budget_limited = true` can be normal on deep ladders
- `bounded_convergence.*.backlog_limited = false` in steady state
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
- repeated cancel-reject cleanup on already-gone orders
- loss of terminal blocked state in API/Signal surfaces
