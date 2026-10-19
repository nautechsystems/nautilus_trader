# Runtime Conformance Contract

Use this reference to locate implementation boundaries and representative checks for selected
[design principles](design_principles.md).

Each section records its source baseline separately. Source links resolve relative to this document's
revision. A baseline identifies the revision used for that section's evidence unless labeled historical. Later
behavior uses the linked source and tests at this document's revision. Neither establishes a guarantee about
future changes.

The entries describe Rust source and test coverage. They do not certify every adapter, Python
entry point, configuration, or failure mode. The named tests are source references, not a record
of a test run.

## Evidence and outcome provenance

Historical source baseline: [46f87cd1b7af576495418761bbf11db23e89124c](https://github.com/nautechsystems/nautilus_trader/commit/46f87cd1b7af576495418761bbf11db23e89124c).
The submission-retention behavior below is covered by the linked source and tests at this document's revision.

The [execution policies](../concepts/execution/policies.md#terminal-reconciliation-provenance)
distinguish venue evidence from local policy resolution. In the Rust live execution manager,
`check_inflight_orders` handles configured retry exhaustion as follows:

- **Submitted orders**: The manager generates a rejection with reason `INFLIGHT_TIMEOUT` when
  submission retention does not apply.
- **Pending updates and cancellations**: The manager generates `OrderCanceled` unless the order was
  never accepted and requires retention.

These events carry `reconciliation=true`. The manager retains unacknowledged orders under
`SubmissionRecoveryPolicy::RetainUnresolved` or when the order's client returns `true` from
`retain_unresolved_submissions`.

- **Implementation**: [Execution manager](../../crates/live/src/execution/manager.rs),
  `check_inflight_orders`, `register_submission_retention`, `submission_is_retained`, and
  `resolve_missing_order`.
- **Representative checks**: [Manager integration tests](../../crates/live/tests/integration/manager.rs),
  `test_inflight_order_generates_rejection_after_max_retries`,
  `test_retained_submission_survives_recovery_and_accepts_late_evidence`,
  `test_check_open_orders_submitted_missing_at_venue_obeys_recovery_policy`,
  `test_inflight_pending_update_generates_canceled`, and
  `test_inflight_pending_cancel_generates_canceled`.
- **Limit**: Retry exhaustion does not establish a venue outcome. The reconciliation flag alone
  does not distinguish a venue report from a local policy resolution, and `OrderCanceled` has no
  reason field. Consumers need the associated inputs and logs to retain that distinction.

## Callback ordering and ownership

Source reviewed at: [16f2163a3cefd5ae212cbddda7dc5253f8fe0bbc](https://github.com/nautechsystems/nautilus_trader/commit/16f2163a3cefd5ae212cbddda7dc5253f8fe0bbc).

The [callback dispatch contract](callback_dispatch.md) requires publication order across recipients and
exclusive component access. Private Rust primitives reserve publication order and reject overlapping checked
access to an allocation. Production callback delivery does not use these primitives. Backtests use the
boundary drain and callback teardown. Live running loops use bounded drains before selecting another event,
with yielding and stop checks between pending batches.

The runtime owns scheduling at a small set of explicit ownership boundaries. Startup and shutdown paths still
need ownership and scheduling coverage before activation. This does not require a drain in each lifecycle or
flush method.

- **Live startup**: After successful startup, the first running-loop drain is the existing delivery boundary.
- **Live disposal**: Releases the retained runner after kernel disposal, then attempts callback cleanup.
  External roots can still block clearing.
- **Standalone lifecycle**: `start`/`stop` has no continuous queued callback delivery schedule and
  retains its existing synchronous behavior.

The [live lifecycle contract](callback_dispatch.md#live-startup-and-standalone-lifecycle) restricts
initial queued activation to Rust `run`/`run_with_mode` and Python `run`/`run_async`. Activation must
reject queued delivery with standalone startup before admitting callbacks. That rejection is not implemented.

- **Implementation**:

  | Source                                                       | Symbols                                              |
  | ------------------------------------------------------------ | ---------------------------------------------------- |
  | [Dispatch](../../crates/common/src/actor/dispatch.rs)        | `PublicationScope` and `drain`.                      |
  | [Allocation access](../../crates/common/src/actor/access.rs) | `AllocationGuard`.                                   |
  | [Live bounded drain](../../crates/live/src/dispatch.rs)      | `drain_callbacks` wrapping `actor::drain_callbacks`. |
  | [Runner](../../crates/live/src/runner.rs)                    | `AsyncRunner::run`.                                  |
  | [Live node](../../crates/live/src/node/mod.rs)               | `run_with_mode` and `dispose`.                       |

- **Representative checks**:

  - `nested_publication_reserves_all_outer_recipients` in the dispatch module checks outer-recipient ordering
    across a nested publication.
  - `test_actor_and_component_views_share_access` in the access module checks exclusion across views.
  - `test_runner_callback_failure_stops_before_next_command` in the runner module checks failure propagation
    and retention of pending messages and the fatal latch.
  - `test_runner_stop_preserves_messages_and_channel_only_scheduling` in the runner module checks ordered
    reuse after stop without an added yield for channel-only work.
  - `test_callback_failure_stops_later_live_events` in the live node module checks failure behavior. Failure
    must stop later event dispatch and the trader, and preserve the latch until disposal.
  - `test_dispose_releases_retained_callback_roots` in the live node module checks runner ownership release
    and fatal latch cleanup. It also checks rejection while external roots exist, and disposal error
    diagnostics.
  - `test_dispose_releases_stop_generated_callback_roots` in the live node module checks that disposal
    releases stop-generated rooted messages without delivering them.
- **Limit**: These checks do not establish production callback ordering or ownership safety. Runtime
  integration must end enclosing mutable borrows before draining and preserve native, Python, and
  dynamic-backend lifecycle eligibility. Unchecked access stays outside the private allocation guards.

  Before activation, exact callback sequence tests through real native and Python components must establish
  deterministic order. This requirement applies to the synchronous core and backtesting. Coverage must include
  nested publication, fan-out, callback-generated commands, same-timestamp work, and continuation across
  bounded passes. Final counts and balances alone do not prove ordering. See the [activation
  requirements](callback_dispatch.md#activation-requirements).

## Recovery

Source baseline: [46f87cd1b7af576495418761bbf11db23e89124c](https://github.com/nautechsystems/nautilus_trader/commit/46f87cd1b7af576495418761bbf11db23e89124c).

For a Rust live node with execution reconciliation enabled, startup performs reconciliation before starting
trader components. A reconciliation error aborts startup. The startup integration test below supplies terminal
order and fill reports through a test execution client. It checks the recovered quantity, price, trade
identity, commission, position quantity, and terminal status.

- **Implementation**: [Live node](../../crates/live/src/node/mod.rs),
  `perform_startup_reconciliation` and its callers.
- **Representative check**: [Node integration tests](../../crates/live/tests/integration/node.rs),
  `test_live_node_startup_recovers_terminal_fill_exactly`.
- **Limit**: This check covers report-based startup recovery, not backing-store durability, supervisor
  restart, or arbitrary panic recovery. Reconciliation can be disabled. Event-store replay also skips live
  client connection and reconciliation. Venue completeness stays subject to the [reconciliation
  policies](../concepts/execution/policies.md#reconciliation-authority).

## Overload handling

Source reviewed at: [16f2163a3cefd5ae212cbddda7dc5253f8fe0bbc](https://github.com/nautechsystems/nautilus_trader/commit/16f2163a3cefd5ae212cbddda7dc5253f8fe0bbc).

The [live runner](../concepts/live.md#dispatch-priority-and-overload-behavior) uses unbounded message
channels. Polling priority does not impose producer backpressure or a queue-depth limit. The private
callback dispatcher separately enforces retained-count, known-storage, and callback-chain limits.

- **Implementation**: [Runner](../../crates/live/src/runner.rs), `AsyncRunner::new` and `recv`.
  [Dispatch](../../crates/common/src/actor/dispatch.rs): admission accounting and `drain`.
- **Representative checks**: `test_recv_processes_system_event_before_command` in the runner
  module checks priority for that channel pair. `event_count_limit_latches_after_exact_capacity`
  and `progress_limit_persists_between_bounded_drains` in the dispatch module check private limits.
- **Limit**: A polling-order test does not prove bounded latency or progress under sustained load.
  Private callback limits do not bound live runner queues or total process memory. Production
  callback activation and live lifecycle ownership coverage remain integration requirements despite
  the implemented running-loop drains. Queue monitoring supplies operational signals without
  automatically throttling feeds or stopping trading.
