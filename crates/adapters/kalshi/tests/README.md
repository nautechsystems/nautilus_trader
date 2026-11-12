# Kalshi conformance

Run these checks from the repository root. They map the data and execution testing specifications to
local peers. Demo DataTester and ExecTester runs stay outside this file.

See the [data-testing specification](../../../../docs/developer_guide/spec_data_testing.md), the
[execution-testing specification](../../../../docs/developer_guide/spec_exec_testing.md), and the
[integration guide](../../../../docs/integrations/kalshi.md).

## Commands

Native library and integration tests, including the cross-boundary schedule and fresh-process
comparison:

```bash
CARGO_BUILD_JOBS=16 cargo test --locked -p nautilus-kalshi --lib --test integration -- \
  multi_market_connection_schedule_matches_independent_oracle \
  book_stream_recovers_gaps fresh_process_cutoff conformance_schedule_preserves \
  --test-threads=8
```

Simulation leg. `simulation` and `--cfg madsim` are both required:

```bash
CARGO_BUILD_JOBS=16 RUSTFLAGS="--cfg madsim ${RUSTFLAGS:-}" \
  cargo test --locked -p nautilus-kalshi --no-default-features --features simulation \
  --lib --test integration -- fresh_process_cutoff multi_market_connection_schedule \
  --test-threads=4
```

Python boundary. The test binary needs the active interpreter's `libpython` on the loader path:

```bash
CARGO_BUILD_JOBS=16 cargo test --locked -p nautilus-kalshi --features python \
  --test integration python_extracted_data_client_requests_the_configured_peer -- \
  --test-threads=1
```

Static DST gate:

```bash
bash .pre-commit-hooks/check_dst_conventions.sh
```

`rest::fresh_process_cutoff_transcripts_match` spawns two fresh processes with
`MADSIM_TEST_SEED=7` and compares the public cutoff request line and parsed fixture domain. A third
process changes `market_settled_ts` by one nanosecond and must not match. The transcript has no
timestamp, signature, or scheduler-dependent field, so equal output does not by itself prove that
scheduling is seed-stable. Signed request fields are pinned by the authenticated HTTP tests.
RSA-PSS signatures are not seed-stable.

## Integrating schedule

`execution_stream::conformance_schedule_preserves_order_totals_reports_and_teardown` runs one native
schedule. The data client and the execution client use separate local peers:

1. Data client bootstrap, one exact quote, and one L2 snapshot checked against the raw book fixture.
2. A stale book SID publishes nothing.
3. One GTC buy fill, in submit, accept, fill order, with exact cash, position, commission, and
   client-order context.
4. Order, fill, and position reports contain that order.
5. After disconnect, neither client sends another request or command.

WebSocket simulation is not part of this schedule. Production rejects Tungstenite, and Sockudo is
native-only.

## Data cases

| Case                             | Result         | Evidence                                                                                                                                                                                     |
| -------------------------------- | -------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| TC-D01, TC-D03                   | Proven         | `instruments.rs` discovery tests; `data::explicit_requests_refresh_context_and_preserve_correlation_filters_and_bounds`                                                                      |
| TC-D02                           | Not applicable | `subscribe_instrument` is unsupported. Definitions refresh over REST and lifecycle.                                                                                                          |
| TC-D10, TC-D14                   | Proven         | `data::book_stream_recovers_gaps_without_false_multi_market_or_control_gaps`; `schedules_match_independent_publication_model`; `multi_market_connection_schedule_matches_independent_oracle` |
| TC-D11                           | Engine-owned   | No native interval channel. Managed L2 deltas are TC-D14. Interval books belong to the data engine.                                                                                          |
| TC-D12                           | Not applicable | `subscribe_book_depth` is unsupported. REST depth is a snapshot limit, not a subscription.                                                                                                   |
| TC-D13                           | Proven         | `data::rest_book_is_an_exact_correlated_response_without_stream_output`                                                                                                                      |
| TC-D20, TC-D30                   | Proven         | `data::lifecycle_streams_selected_types_and_shares_demand_across_reconnect`                                                                                                                  |
| TC-D21                           | Not applicable | `request_quotes` returns `Kalshi does not provide historical quotes`.                                                                                                                        |
| TC-D31, TC-D41                   | Proven         | `history.rs` trade and bar request tests                                                                                                                                                     |
| TC-D40                           | Not applicable | `subscribe_bars` is unsupported. Historical candles are TC-D41.                                                                                                                              |
| TC-D50 to TC-D53, TC-D62, TC-D63 | Not applicable | Not a derivative, option-chain, or greeks product. The unsupported subscription macros reject them.                                                                                          |
| TC-D60, TC-D61                   | Proven         | `data.rs` lifecycle and settlement tests; `data/lifecycle.rs` unit tests                                                                                                                     |
| TC-D70                           | Proven         | Lifecycle unsubscribe and the conformance schedule's post-disconnect silence                                                                                                                 |
| TC-D71, TC-D72                   | Proven         | Instrument request params in `data.rs`; bar params in `history.rs`                                                                                                                           |
| TC-D73                           | Partial        | Unsubscribe and disconnect release demand. Shared trader retirement is not wired to this adapter.                                                                                            |
| TC-D74, TC-D75                   | Not applicable | No DeFi pool feeds.                                                                                                                                                                          |

## Execution cases

| Case                                                       | Result         | Evidence                                                                                                                            |
| ---------------------------------------------------------- | -------------- | ----------------------------------------------------------------------------------------------------------------------------------- |
| TC-E01 to TC-E06                                           | Not applicable | No native market order. `native_gtc_invalid_instructions_never_submit` denies the input.                                            |
| TC-E10 to TC-E18                                           | Proven         | `native_limit_matrix_recovers_final_state_and_exact_wire`; `native_limit_fill_before_ack_and_duplicate_delivery_apply_once`         |
| TC-E19                                                     | Not applicable | `DAY` is outside the initial order scope. Invalid time-in-force input is denied before submit.                                      |
| TC-E20 to TC-E27, TC-E34, TC-E35, TC-E50 to TC-E53, TC-E62 | Not applicable | No conditional, stop, bracket, or display-quantity orders. Denial tests reject them before dispatch.                                |
| TC-E30, TC-E31, TC-E36                                     | Proven         | `native_change_wire_and_final_state`; `native_modify_rejection_is_scoped_and_clean`                                                 |
| TC-E32, TC-E33                                             | Demo remainder | Spec skip is never. Cancel and submit are each proven. The ExecTester cancel-then-resubmit sequence is not a local test.            |
| TC-E40, TC-E44                                             | Proven         | `native_closed_order_changes_are_denied_before_dispatch`; single-cancel tests                                                       |
| TC-E41 to TC-E43                                           | Command proven | `native_batch_cancel_all_preserves_scoped_peers` and batch cancel tests. Strategy-stop orchestration remains a demo ExecTester run. |
| TC-E60, TC-E61, TC-E63, TC-E70, TC-E71                     | Proven         | Post-only, reduce-only, and custom STP params in `execution_stream.rs`                                                              |
| TC-E72, TC-E73                                             | Proven         | `native_gtc_invalid_instructions_never_submit`; batch validation before dispatch                                                    |
| TC-E74                                                     | Proven         | `native_gtc_failures_preserve_the_evidence_authority` keeps an unknown submit non-terminal.                                         |
| TC-E75, TC-E76                                             | Proven         | `native_change_ambiguity_reconciles_without_replay`; `native_unknown_cancel_publishes_fills_before_resolution`                      |
| TC-E77                                                     | Proven         | `native_batch_ambiguity_retains_each_child_until_authority`                                                                         |
| TC-E78                                                     | Proven         | `native_batch_partial_application_keeps_the_other_child_unresolved`                                                                 |
| TC-E80                                                     | Not applicable | Spec skip is no market orders. Startup reconciliation is TC-E84 to TC-E87, not an opening market order.                             |
| TC-E81                                                     | Command proven | Same scoped cancel command as TC-E41. Strategy-stop orchestration remains a demo ExecTester run.                                    |
| TC-E84 to TC-E89                                           | Proven         | `execution.rs` report tests; `startup_reconciliation_preserves_exact_execution`                                                     |
| TC-E82                                                     | Not applicable | No market close-on-stop. A supported limit close is a separate order.                                                               |
| TC-E83                                                     | Proven         | Disconnect and the conformance schedule leave no post-teardown traffic.                                                             |
| TC-E90 to TC-E101                                          | Not applicable | Option-chain and alternative-pricing cases are not this binary-contract product.                                                    |

## Named behaviors

| Behavior          | Evidence                                                                                                                                 |
| ----------------- | ---------------------------------------------------------------------------------------------------------------------------------------- |
| Shared quota      | `rest::account_callers_share_quota_across_keys_and_transport_lifetimes`                                                                  |
| History cutoff    | `history::trades_collect_live_then_full_archive_with_one_fixed_cutoff`                                                                   |
| Settlement        | `data::missed_lifecycle_settlement_recovers_from_rest`; `data::tests::settlement_notification_rechecks_rest_until_final_or_bounded_stop` |
| Readiness         | `websocket::handler` `running_snapshot_cancellation_restores_readiness_without_a_frame`                                                  |
| Restart           | `execution_stream::startup_reconciliation_preserves_exact_execution`                                                                     |
| Rejected recovery | `book::sync` `snapshot_rejection_reaches_only_its_correlated_recovery`; `rejected_replay_keeps_a_recovery_owner`                         |
| Late snapshot     | `book::sync` `canceled_confirmation_cannot_open_new_attempt_or_accept_late_snapshot`                                                     |

## Book recovery validation

Current validation supports only autonomous live `churn --rounds 0` and `--scenario self-check`.
`churn` defaults to and requires zero rounds. It connects, subscribes, verifies healthy books,
recovers from a shared-SID sequence gap and one dropped recovery snapshot per book without
reconnecting, waits for final independent venue-oracle coverage and incremental publication, and
stops. Self-check requires no credentials or venue connections.

Reconnect-dependent live scenarios are unavailable while transparent Kalshi WebSocket reconnects
are disabled: `soak`, `initial`, `turnover`, `boundaries`, and multi-round `churn`. Update-triggered
and quiet-stream proxy cuts are also unavailable pending the reconnect follow-up.

### Historical live measurements

These measurements apply only to the historical sources identified below. They do not establish
current reconnect capability. The revised transport and harness require fresh live validation after
the reconnect follow-up. Historical reconnect scenarios used update-triggered proxy cuts, so they
required venue update activity and could not force a reconnect on a completely quiet stream.

L0-L4 market-data validation passed on 6 November 2026. Kalshi and shared book production sources matched
`f3f8c491d1635243fb7a9b1e68eca47258167985`. The tested harness source SHA-256 was
`e47574cdc356897c70b7b39426cd824843471370bb33ff74c0a8411392343c55`. The runs submitted no orders.

| Scope             | Scenario   | Timeout (s) | Rounds | Duration (s) | Batches | Verified episodes |
| ----------------- | ---------- | ----------- | ------ | ------------ | ------- | ----------------- |
| Production crypto | churn      | 10          | 12     | 154          | 36,257  | 42                |
| Production crypto | autonomous | 1           | 0      | 8            | 209     | 6                 |
| Production NBA    | churn      | 10          | 12     | 147          | 2,764   | 42                |
| Demo              | initial    | 10          | 5      | 96           | 21      | 15                |
| Demo              | initial    | 1           | 1      | 8            | 3       | 3                 |
| Demo              | initial    | 0           | 1      | 81           | 4       | 3                 |
| Demo              | turnover   | 1           | 12     | 120          | 62      | 48                |
| Demo              | boundaries | 1           | 1      | 172          | 39      | 17                |
| Demo              | soak       | 1           | 30     | 605          | 273     | 93                |
| Production NBA    | soak       | 10          | 30     | 607          | 11,265  | 93                |

The historical autonomous probe used `churn --rounds 0`; it performed the same initial gap and
dropped-snapshot recovery without reconnecting. Historical churn rotated sequence gaps, lost snapshots,
held obsolete frames, rejected replacements, reconnect cuts, and shutdown during recovery. Turnover
covered unsubscribe/resubscribe with held, dropped, and rejected snapshots. Boundaries exhausted eight
attempts, verified the one-minute ceiling and reconnect wake, then checked unsubscribe cancellation and
shutdown during reconnect. With timeout zero, the initial probe stayed silent for 70 seconds with no
snapshot requests, then reconnected on the next venue update. The ten-second initial probe passed five
consecutive fresh sessions and waited for incremental publication on the active book. Every historical
reconnect scenario needed venue updates to trigger its proxy cut.

Market scope:

- Production crypto: `KXBTC15M-26NOV052330-30`, `KXETH15M-26NOV052330-30`, and `KXSOL15M-26NOV052330-30`.
- Production NBA churn: `KXNBATOTAL-26NOV05ORLPHX-223`, `KXNBATOTAL-26NOV05MIAGSW-237`, and
  `KXNBATOTAL-26NOV05MIAGSW-235`.
- Production NBA soak: `KXNBATOTAL-26NOV05MIAGSW-239`, `KXNBATOTAL-26NOV05MIAGSW-237`, and
  `KXNBATOTAL-26NOV05MIAGSW-235`.
- Demo: `KXDEMOFUTHOURLY-26NOV06H05-UP`, `KXNBATOTAL-26NOV06PHICLE-231`, and
  `KXNBATOTAL-26NOV06PHICLE-229`.

All 50,897 emitted batches and 362 snapshot episodes match the independent raw-wire oracle at depth 100.
Separate WebSocket connections provide 43,112 comparisons at matching market exchange clocks, including
quiet and empty books. Partial, missing, or evicted samples do not count: 7,755 are skipped, and
30 remain pending at shutdown. Every book has an independent comparison in each session. The runs verify
116 empty and 63 one-sided snapshot episodes. Every qualifying run ends with zero raw-oracle misses,
checker violations, dark books, unexplained errors, upstream failures, or active proxies. Maximum reported
recovery is 60.253 seconds at the deliberate retry ceiling; outside that boundary probe, it is
13.121 seconds. Shutdown takes at most 112 milliseconds.

The shared model passes 2,048 generated schedules. A planted stale recovery owner regression fails.
The harness also detects planted partial clock completion, premature publication readiness, and incorrect
independent levels; restored sources pass. Focused native validation passes 39 Kalshi book tests,
52 shared book tests, and 49 shared harness tests. The harness self-check and focused Clippy check pass.

An earlier NBA soak fails when the ORL-PHX market closes during the run and its exchange clock decreases.
A follow-up probe confirms empty snapshots with clock `0` after finalization. That attempt does not count
toward qualification. The replacement soak uses three MIA-GSW markets that remain active through shutdown.

The log audit identifies two expected retry-budget errors. Other diagnostics cover injected faults and
pre-existing order-book high-water warnings after SID sequence resets or earlier venue timestamps. These
warnings do not coincide with checker or oracle failures. Kalshi treats snapshot errors as retryable;
the shared deterministic suite covers the permanent-error classifier branch.

## Venue-only remainder

These stay for demo validation. Local tests do not claim them:

- DataTester and ExecTester runs for discovery, live quotes, trades, books, history, status, limit
  time-in-force, submit, amend, decrease, cancel, batch, account state, and restart.
- Private-stream loss around fills, and restart with external orders or open positions.
- Production validation of other data streams. Funded production orders need a separate authorization.
