# High-impact TODO triage (initial)

This document lists high-impact areas with TODO/FIXME markers and suggested follow-ups.

## System/kernel
- File: crates/system/src/kernel.rs — multiple TODOs around scheduler, shutdown paths, backpressure.
- Action: write explicit state machine for lifecycle (init→start→quiesce→stop), add property tests around shutdown ordering.

## Data engine
- File: crates/data/src/engine/mod.rs — backpressure, fanout, error handling TODOs.
- Action: introduce bounded channels with metrics; test with proptest for ordering under burst.

## Execution/matching engine
- Files: crates/execution/src/matching_engine/engine.rs, matching_core/mod.rs — edge-case TODOs.
- Action: cover partial fills, cancel/replace races, trailing updates with property tests.

## Portfolio/positions
- Files: crates/portfolio/src/{manager.rs, portfolio.rs} — TODOs on locking and invariants.
- Action: add invariants on balances/locks; add proptests around cash/margin updates.

## Common/actor runtime
- Files: crates/common/src/actor/* — TODOs on timeouts and supervision.
- Action: supervision tree policy; jittered backoff; tests for restart strategies.

## Networking/ws
- Files: crates/network/src/websocket.rs — TODOs on reconnect and message ordering.
- Action: reconnection state machine tests; fuzz framing parser.

## Serialization/Arrow
- Files: crates/serialization/src/arrow/* — TODOs around schema evolution and nullability.
- Action: add roundtrip tests across versions; document schema compat policy.

## Python API surface
- Files: services/api/main.py — complexity and S* lint ignores.
- Action: split routes into modules; add type hints on payloads; keep ASGI tests.

## Next steps
- Convert each bullet to a GitHub issue and link the line ranges.
- Gate new unwrap/expect in CI (already reporting). Gradually reduce counts per area.
