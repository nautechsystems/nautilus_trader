# TokenMM Binance Private-Path Safety PR1 Design

**Date:** 2026-03-31

## External Review Context

This document is intended to stand on its own without chat history.

The incident statement that motivated this work is directionally correct:

1. Binance private execution/account freshness was degraded for a long time before the bots stopped.
2. March 31, 2026 was a late stop event, not the real start of the incident.
3. The important production requirement is to make this class of failure visible, bounded, non-silent, and fail-closed in the right place.

There is one important architecture correction, and it changes PR1 scope materially:

1. Live TokenMM Binance strategies do **not** write through `nautilus_trader/adapters/binance/execution.py` in production.
2. `systems/flux/flux/runners/tokenmm/run_node.py` disables local execution for controller-managed Binance strategies and attaches `_attach_controller_managed_binance_bridge`.
3. The active write path is `run_node.py -> UDS controller -> _TokenmmBinanceRequestBoundVenueWriter -> BinanceSpotAccountHttpAPI / BinanceFuturesAccountHttpAPI -> BinanceHttpClient`.
4. Shared Binance account freshness for TokenMM balances also goes through the same cached HTTP client family in `systems/flux/flux/runners/shared/profile_accounts.py`.

That means a PR scoped only to the generic Binance execution adapter and websocket user-stream client would be a useful follow-up for non-TokenMM paths, but it would miss the current prod TokenMM write path.

## System Context

For this PR, the relevant runtime surfaces are:

- `nautilus_trader/adapters/binance/http/client.py`
  This is the shared Binance HTTP wrapper used by both the TokenMM controller write path and the shared-account projection providers.
- `nautilus_trader/adapters/binance/factories.py`
  This is where the cached Binance HTTP client is constructed for adapter consumers.
- `systems/flux/flux/runners/tokenmm/run_controller.py`
  This is the production TokenMM Binance place/cancel path. It builds request-bound Binance HTTP writers and publishes canonical controller state back to the strategy.
- `systems/flux/flux/runners/shared/profile_accounts.py`
  This produces shared Binance account projection payloads and already carries `projection_status` metadata, but it does not fully wire the configured Binance HTTP timeout knob.
- `systems/flux/flux/runners/tokenmm/run_node.py`
  This is the seam where controller-managed state can be attached to MakerV3 strategies.
- `systems/flux/flux/strategies/makerv3/*.py`
  This is where quote refresh must block early and publish an honest blocked reason instead of burning through quote-failure retries.

## Review Questions

External review should focus on these questions:

1. Does PR1 fix the actual prod TokenMM Binance path instead of a similar but inactive adapter path?
2. Does it turn opaque transport timeouts into explicit private-path health state with bounded behavior?
3. Does it block quoting earlier than the current quote-fail circuit-breaker path?
4. Does it add minimal operator-visible state without starting a full health-domain architecture rewrite?
5. Is the rollout small enough to ship quickly with low regression risk?

## Goal

Ship one narrow incident-hardening PR that makes the TokenMM Binance private HTTP/account path explicitly timed, classified, surfaced, and fail-closed for quoting, without starting the broader health-model wave.

The immediate production target is:

- the live TokenMM Binance controller path uses a real HTTP timeout knob
- raw timeout failures are classified distinctly from generic venue errors
- repeated private-path timeout/staleness blocks new quoting before MakerV3 trips the quote-failure circuit
- operators can see that the Binance private path is stale from existing controller/state surfaces

## Current Failure

The production failure mode is currently unsafe in four ways:

1. The shared Binance HTTP client supports request timeouts, but the Binance wrapper/factory path does not fully wire a real timeout knob through the TokenMM controller and shared-account projection callers.
2. Raw transport `TimeoutError` is not treated as a first-class Binance private-path failure in the controller-managed TokenMM path.
3. The strategy learns about the failure too late. It reaches quote refresh and eventually trips the quote-failure circuit instead of entering an earlier, explicit blocked state.
4. The operator surface does not carry one crisp private-path health payload for TokenMM-managed Binance strategies, so the problem remains too silent until the strategy stops itself.

## Options

### Option 1: Patch only the generic Binance execution adapter and websocket user-stream lifecycle

This is not the right PR1.

It would improve the generic Binance adapter, but live TokenMM Binance writes currently bypass that adapter. On its own, it does not cover the controller-managed prod write path that actually times out today.

### Option 2: Patch the shared Binance HTTP client plus the TokenMM controller-managed private path

This is the recommended option.

It keeps the blast radius tight while hitting the real production path:

- one timeout knob that actually works
- one timeout classification path
- one controller-published private-path health payload
- one strategy-side early block

### Option 3: Start the broader health-domain split now

This is explicitly out of scope for PR1.

It is directionally good, but it would turn an incident hardening patch into an architecture discussion and expand the review surface far beyond what prod needs immediately.

## Recommendation

Implement Option 2.

PR1 should be framed as:

> Do not let controller-managed TokenMM Binance strategies keep quoting into a stale or timing-out private HTTP/account path, and do not let raw transport timeouts remain unclassified.

## Design Principles

1. Fix the real prod path first.
2. Prefer one narrow health signal over a new generalized health framework.
3. Reuse existing payload patterns where possible, especially `projection_status`-style fields and controller canonical state.
4. Block earlier than the quote-fail circuit breaker.
5. Keep the public/operator contract additive and minimal.

## PR1 Scope

PR1 includes exactly five work items:

1. Wire a real Binance HTTP timeout knob through the shared HTTP client path that TokenMM actually uses.
2. Classify raw `TimeoutError` explicitly as a private-path transport failure in the TokenMM controller and shared-account projection path.
3. Publish one narrow private-path health payload from the controller-managed Binance path.
4. Gate MakerV3 quote refresh on that private-path health payload so quoting stops before quote-failure escalation.
5. Add focused tests and minimal state/alert visibility.

## Detailed Design

### 1. Wire `http_timeout_secs` Through The Shared Binance HTTP Path

`BinanceHttpClient` already sits under both:

- the TokenMM controller Binance request writer
- the shared Binance account projection providers

PR1 should add explicit timeout plumbing so the configured timeout reaches the underlying `HttpClient`.

Concretely:

- `nautilus_trader/adapters/binance/http/client.py` should accept and store `timeout_secs`, and pass it through to the underlying pyo3 `HttpClient` and request calls.
- `nautilus_trader/adapters/binance/factories.py` should accept the timeout when building cached Binance HTTP clients.
- `systems/flux/flux/runners/shared/profile_accounts.py` should pass the already-supported `AccountScopeConfig.http_timeout_secs` into the Binance provider client construction.
- `systems/flux/flux/runners/tokenmm/run_controller.py` should read a venue-level `http_timeout_secs` from managed strategy runtime configs and pass it into the controller writer's Binance HTTP client construction.

This removes the dead-knob problem and makes controller write timeouts and shared-account refresh timeouts actually configurable.

### 2. Classify Raw Transport Timeouts Explicitly

PR1 should not treat transport timeouts as generic opaque exceptions.

Instead:

- add one narrow Binance timeout classification helper or exception type in the shared Binance HTTP layer
- preserve the original exception class/name for logs and tests
- use that classification in:
  - TokenMM controller write failures
  - shared Binance account projection refresh failures

The goal is not a new Binance-wide error taxonomy. The goal is to distinguish:

- retryable/private transport timeout
- terminal venue/business rejection
- generic unexpected exception

in the path that prod TokenMM actually uses.

### 3. Add A Controller-Published Private-Path Health Snapshot

PR1 needs one small health object, not a full health framework.

The controller canonical state for managed Binance strategies should gain an additive payload such as `private_path_health` with fields like:

- `state`: `ok` or `stale`
- `healthy`: boolean
- `last_success_ts_ms`
- `last_attempt_ts_ms`
- `stale_after_ms`
- `timeout_count`
- `last_error_type`
- `last_error_message`

This payload should update when:

- a controller-managed Binance write succeeds
- a controller-managed Binance write times out
- shared-account projection refresh records repeated timeout/stale behavior for the same account scope

The naming should stay close to existing `projection_status` fields so operators do not have to learn a second style for the same concept.

### 4. Gate MakerV3 On Private-Path Staleness Before Quote-Fail Escalation

PR1 should add a narrow strategy-side gate only for the controller-managed path.

Mechanically:

- `run_node.py` controller bridge should sync the additive `private_path_health` payload from controller canonical state onto the strategy.
- MakerV3 should read that payload during quote refresh.
- If the private path is stale, MakerV3 should:
  - stop new quote placement/amend activity
  - cancel working quotes if required by the existing blocked-state semantics
  - publish an explicit blocked reason
  - avoid counting this as another generic quote-refresh failure that only feeds the quote-fail circuit breaker

This is intentionally narrower than a full market/private/account health split. It is one extra fail-closed blocker for the real incident path.

### 5. Add Minimal Visibility

PR1 should expose the new failure class through existing surfaces with minimal change:

- controller canonical state carries `private_path_health`
- MakerV3 state payload carries `private_path_health`
- MakerV3 emits a specific blocked reason code, for example `blocked_private_path_stale`
- TokenMM alerts/readiness surfaces should render that state as a current actionable warning instead of waiting for a later stale-state or quote-fail symptom

This is enough for PR1. Richer health-domain split and broader operator UX remain follow-up work.

## Non-Goals

These items are intentionally out of scope for PR1:

- a full market-data/private-execution/account-state health-domain model
- generic Binance websocket user-stream lifecycle redesign across all non-TokenMM paths
- a new standalone health service
- cross-venue health unification

## Testing Strategy

PR1 must prove four things:

1. configured Binance HTTP timeouts are actually threaded through the shared client path
2. controller and shared-account refresh timeout failures are classified and surfaced explicitly
3. controller canonical state carries the new private-path health payload
4. MakerV3 blocks quoting on stale private-path health without tripping the generic quote-fail circuit

The primary regression suites are:

- `tests/integration_tests/adapters/binance/test_factories.py`
- `tests/unit_tests/flux/runners/shared/test_profile_accounts.py`
- `tests/unit_tests/examples/strategies/test_tokenmm_run_controller.py`
- `tests/unit_tests/examples/strategies/test_tokenmm_run_node.py`
- `tests/unit_tests/flux/strategies/makerv3/test_quote_engine.py`
- `tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py`
- `tests/unit_tests/examples/strategies/test_tokenmm_run_api.py`

## Rollout Notes

This PR is intended to be small enough for a fast pilot/prod wave:

1. deploy the code patch with explicit `http_timeout_secs` set on the Binance TokenMM strategy/runtime configs and Binance shared account scope
2. validate that controller canonical state and strategy state show `private_path_health`
3. validate that simulated or induced Binance timeout conditions produce an early blocked state rather than a later quote-fail stop

## Follow-Up After PR1

After PR1 lands, the next wave can decide whether to:

- extend the same timeout classification into the generic Binance execution adapter and websocket user-stream client
- split health into market/private/account domains formally
- standardize operator UX across venues

Those remain separate from this safety patch by design.
