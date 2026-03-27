# Shared Deque Quote Stack Design

**Goal:** Replace MakerV3's current bounded-convergence stack maintenance with a simpler shared deque-style stack planner that preserves production safety, allows temporary `N+1` during inward moves, and makes middle-of-stack cancel/replace impossible in normal repricing.

**Architecture:** Keep pricing and target-generation responsibilities where they already live, but move stack-management policy into a new pure shared planner module. The planner works one side at a time over ordered active levels and ordered desired levels, and only emits front/back mutations plus explicit hole repair. Telemetry completion is part of scope for the same PR: if `order_action` or `quote_cycle` is missing the reason/level fields needed to prove the new behavior, this PR fills that gap instead of deferring it.

**Tech Stack:** Python, Nautilus Trader strategy runtime, Flux MakerV3, shared strategy utilities, SQLite order/quote-cycle telemetry, pytest.

## Problem Summary

The current MakerV3 stack behavior is more complicated than the desired contract:

- `max_age_ms` is currently reused as both market-data staleness and resting-order TTL, so steady-state quoting can churn even when fair value has not moved.
- The current planner explicitly allows stale matched-level replacement and matched-level peeling for room creation.
- Those behaviors make deliberate middle-of-stack cancel/replace possible.
- TokenMM and equities do not currently share stack-management logic because `equities_maker` is still a thin `MakerV4Strategy` wrapper with a one-maker-order-per-side runtime.
- The live Bybit telemetry surface is incomplete today: `quote_cycle` still carries enough signal to audit stack behavior, but `order_action` rows do not currently persist the documented intent enrichment fields needed for clean lifecycle proof.

The target contract is simpler: normal quote maintenance should behave like a deque.

## Approaches Considered

### 1. Recommended: New pure shared deque planner

Create a new shared planner module that receives the ordered active stack and ordered desired stack for one side and returns a small ordered action list. Normal repricing uses only front/back mutations. Hole repair is allowed when the venue/cache truth shows a missing level, but no matched middle order is canceled for repricing.

Pros:

- Matches the intended mental model exactly.
- Much easier to reason about and test.
- Naturally limits churn without relying on advanced rate knobs.
- Creates a reusable shared stack-management module for any future multi-level strategy family.

Cons:

- Requires replacing the current bounded-convergence planner path in MakerV3 rather than patching it in place.

### 2. Minimal patch to current bounded-convergence planner

Keep the existing planner shape but remove `cancel_stale_order` and `cancel_free_slot_for_missing_level`, then trim the remaining action types down toward front/back-only behavior.

Pros:

- Smaller initial diff.
- Lower immediate risk of integration fallout.

Cons:

- Leaves a more complicated planner in place than the intended contract needs.
- Keeps alignment/index semantics that are already hard to audit.
- More likely to leave edge cases hiding in the old structure.

### 3. Stateful slot engine

Track explicit per-side slot identities and slide them as fair value moves.

Pros:

- Very explicit queue semantics.

Cons:

- More stateful than necessary.
- Harder to reconcile against venue/cache truth.
- Adds moving parts without improving the user-visible behavior beyond the recommended approach.

## Recommended Decision

Use Approach 1.

Build a new shared pure planner and integrate MakerV3 onto it first. Do not attempt to unify the whole runtime with equities in this PR. Share only the planner contract. `equities_maker` can adopt the shared planner later if and when it becomes a true multi-level resting-stack strategy.

## Intended Stack Contract

The planner runs per side over ordered prices from best to worst.

### Inputs

- ordered active managed orders for one side
- ordered desired levels for one side
- target depth `N`
- per-level `place_px`, `cancel_px`, `match_tol`
- side state flags:
  - pending cancel present
  - pending place present if tracked
  - side blocked
- post-only/placeability result for any candidate new price

### Outputs

- ordered actions for the current tick
- per-side diagnostics for telemetry

Allowed actions:

- `cancel_front`
- `cancel_back`
- `place_level(level_index)`
- `no_op`

Disallowed normal-path actions:

- cancel matched middle level because it is old
- cancel matched middle level to create room
- broad multi-level resnapshot

### Normal Quote Tick Behavior

Per side:

1. Sort active orders best to worst.
2. Cancel obvious invalid state first.
   - Cancel front orders that violate their cancel edge.
   - Cancel back orders when the stack depth is greater than `N`.
3. Freeze structural repricing for that side if the side already has pending cancel/place work in flight.
4. If the stack is short, repair by placing the most aggressive missing desired level.
   - If the top is missing, place the top.
   - If the top is intact and only the tail is missing, place the tail.
   - If there is a real interior hole from a fill or external disappearance, repair the most aggressive missing desired level, but never cancel another order to do it.
5. If a more aggressive new front level is desired and placeable:
   - Place the new front level.
   - Allow temporary `N+1`.
   - Then cancel the back level.
6. If the front is now too aggressive:
   - Cancel the front.
   - If depth is now short and the new tail is placeable, place the tail.
7. Limit normal repricing to one structural deque step per side per tick.

Examples:

- Bid stack moves inward: `place_front`, then `cancel_back`.
- Bid stack moves outward: `cancel_front`, then `place_back`.
- Stable fair value: `no_op`.
- Missing level after fill: `place_level(missing_level_index)` with no paired repricing cancel.

This preserves the desired visual behavior:

- fair value up on bids: add at the front, trim from the back
- fair value down on bids: trim from the front, refill at the back
- no deliberate middle-of-stack cancel/replace

## Hole Repair Policy

Normal repricing should never touch the middle, but production reality still needs a hole-repair rule.

If a fill, external cancel, or reconciliation event removes a level from the middle:

- do not cancel a different resting order to make the stack look pretty
- place the most aggressive missing desired level that restores the ordered stack
- count this separately in telemetry as `repair_hole`

This allows the strategy to recover from real venue/counterparty events without reintroducing middle-of-stack repricing churn.

## Parameter Policy

The operator-facing quoting surface should become smaller in practice, not larger.

Keep using the existing quoting inputs that actually describe the ladder:

- edges
- distances
- order counts / band shape
- bot/risk controls
- market-data staleness

Change the meaning of nuanced convergence controls:

- `max_age_ms` remains market-data freshness only
- resting-order TTL is removed from normal quote maintenance
- no new operator-facing "advanced convergence" knobs are added

Compatibility rule for this PR:

- existing advanced convergence fields may remain accepted on config/runtime surfaces to avoid config breakage
- the new deque planner should not rely on them for ordinary quote maintenance behavior

This keeps the runtime surface stable for deployment while simplifying the behavior traders actually observe.

## Production-Grade Safety That Stays

This PR changes stack policy, not the existing production safety posture.

Keep:

- stale/unavailable market-data blocking and safety cancels
- pending-cancel tracking and side freeze
- cancel-reject cooldown
- startup cleanup and reconciliation
- post-only clamp
- unique-price nudging
- existing risk and bot-on gates
- venue-protection hard stop behavior

Remove from normal stack maintenance:

- matched-order TTL refresh
- matched-level peeling to create room
- multi-level resnapshot behavior hidden inside bounded convergence

## Shared Module Boundary

Create a new pure shared planner module:

- `systems/flux/flux/strategies/shared/quote_stack.py`

The shared module owns:

- deque-style stack action planning
- stack matching helpers
- stack diagnostics payload building

It does not own:

- pricing/edge calculation
- venue-specific post-only clamping
- order submission/cancel side effects
- market-data stale blocking
- risk/bot gates

This keeps the shared module portable and easy to reuse.

## Telemetry Requirements

Telemetry completion is in scope for the same PR.

### Quote-Cycle

Per side, publish explicit stack-action diagnostics:

- `stack_action_mode`
  - `no_op`
  - `place_front_cancel_back`
  - `cancel_front_place_back`
  - `place_missing`
  - `cancel_front`
  - `cancel_back`
  - `repair_hole`
- `front_changed`
- `back_changed`
- `depth_before`
- `depth_after`
- `missing_level_count`
- `interior_hole_count`

### Order Intent / Order Action

If the live path is missing them, this PR must ensure persisted lifecycle rows carry:

- `reason_code`
- `level_index`
- enough correlation to tie actions back to `quote_cycle`

Minimal reason taxonomy for the new path:

- `cancel_front_violation`
- `cancel_back_excess`
- `place_front_improve`
- `place_back_backfill`
- `place_missing_hole_repair`

The outcome requirement is practical: after this PR, a quick SQLite query should be able to prove that normal repricing only touched the front/back.

## Rollout Strategy

Land the deque planner behind the current MakerV3 strategy family first.

Rollout sequence:

1. Add pure planner tests first.
2. Integrate MakerV3 onto the planner while keeping existing safety gates.
3. Fix telemetry completion in the same PR if required.
4. Validate on TokenMM through `quote_cycle` and `order_action` queries that show front/back-only mutations.
5. Revisit whether equities should adopt the shared planner in a follow-up PR after the planner is stable and the strategy family direction is clear.

## Success Criteria

- Stable fair value produces mostly `no_op`.
- Normal repricing never deliberately cancels a matched middle order.
- Inward moves visibly behave as `place front, cancel back`.
- Outward moves visibly behave as `cancel front, place back`.
- Hole repair works without interior repricing cancels.
- `quote_cycle` explicitly records the deque action type.
- `order_action` persists enough metadata to prove why each quote action happened.

## Non-Goals

- Do not redesign the ladder economics.
- Do not redesign risk logic.
- Do not refactor current equities MakerV4 into a multi-level stack in this PR.
- Do not add a large new matrix of tuning knobs for quoting behavior.

## Approved Assumptions

- Temporary `N+1` is allowed during inward moves.
- Less operator-visible tuning is better.
- Telemetry completion belongs in the same PR if missing.
- Reasonable compatibility shims are acceptable if they prevent deploy/config churn while simplifying actual behavior.
