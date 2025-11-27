# Recommended GitHub Issues for NautilusTrader

**Generated**: November 27, 2025
**Priority Levels**: P0 (Critical), P1 (High), P2 (Medium), P3 (Low)

---

## Table of Contents

1. [P0 - Critical Issues](#p0---critical-issues)
2. [P1 - High Priority Issues](#p1---high-priority-issues)
3. [P2 - Medium Priority Issues](#p2---medium-priority-issues)
4. [P3 - Low Priority Issues](#p3---low-priority-issues)

---

## P0 - Critical Issues

### Issue #1: Risk Engine Multi-Venue Bypass

**Title**: `[CRITICAL] Risk engine bypasses all checks when venue account not found`

**Labels**: `critical`, `security`, `risk-engine`, `bug`

**Description**:
The risk engine currently returns `true` (allowing order) when it cannot find an account for a specific venue. This means **all risk controls are bypassed** for multi-venue trading scenarios.

**Location**:
- File: `crates/risk/src/engine/mod.rs`
- Lines: 617-628

**Current Code**:
```rust
let account_exists = {
    let cache = self.cache.borrow();
    cache.account_for_venue(&instrument.id().venue).cloned()
};

let account = if let Some(account) = account_exists {
    account
} else {
    log::debug!("Cannot find account for venue {}", instrument.id().venue);
    return true; // TODO: Temporary early return until handling routing/multiple venues
};
```

**Impact**:
- Orders can execute without balance verification
- No notional limits enforced
- Cross-venue positions can exceed total allowed margin
- Regulatory compliance at risk

**Acceptance Criteria**:
- [ ] Implement multi-venue account aggregation logic
- [ ] Add fallback account resolution for routing scenarios
- [ ] Deny orders when no valid account can be determined
- [ ] Add comprehensive test coverage for multi-venue scenarios

**Estimated Effort**: 3-5 days

---

### Issue #2: Risk Engine Margin Account Controls Not Implemented

**Title**: `[CRITICAL] Zero risk controls applied to margin accounts`

**Labels**: `critical`, `security`, `risk-engine`, `bug`

**Description**:
When an account is a `MarginAccount`, the risk engine immediately returns `true`, bypassing **ALL** subsequent risk checks including leverage limits, margin requirements, and free balance verification.

**Location**:
- File: `crates/risk/src/engine/mod.rs`
- Lines: 629-632

**Current Code**:
```rust
let cash_account = match account {
    AccountAny::Cash(cash_account) => cash_account,
    AccountAny::Margin(_) => return true, // TODO: Determine risk controls for margin
};
```

**Impact**:
- Margin accounts have NO risk validation
- Leverage limits not enforced
- Initial/maintenance margin not verified
- Liquidation risk not assessed
- Can cause account liquidation without warning

**Evidence**: Test at line 3306-3363 explicitly documents this bypass:
```rust
// Currently, it executes because check_orders_risk returns true for margin_account
```

**Acceptance Criteria**:
- [ ] Implement `check_margin_account_risk()` function
- [ ] Add leverage limit validation
- [ ] Add initial margin requirement checks
- [ ] Add maintenance margin verification
- [ ] Add free margin availability checks
- [ ] Add liquidation risk assessment
- [ ] Fix failing test at line 3306
- [ ] Add margin-specific configuration to `RiskEngineConfig`

**Estimated Effort**: 5-7 days

---

### Issue #3: Order Emulator Bypasses Risk Engine on Order Release

**Title**: `[CRITICAL] Emulated orders bypass risk engine when triggered`

**Labels**: `critical`, `security`, `risk-engine`, `order-emulator`, `bug`

**Description**:
When emulated orders (stop-loss, trailing stops, etc.) are triggered, they are sent directly to the execution engine, **completely bypassing** the risk engine validation. This means large orders can execute without notional checks, balance verification, or margin validation.

**Location**:
- File: `crates/execution/src/order_emulator/emulator.rs`
- Lines: 978-983 (limit orders), 1089-1094 (market orders)

**Current Flow**:
```
Strategy → OrderEmulator (NO risk validation)
         → Trigger activated
         → ExecEngine (BYPASSES RiskEngine)
```

**Impact**:
- Stop-loss orders can exceed available balance
- Trailing stop orders can violate notional limits
- Margin requirements not checked on release
- Can cause unexpected liquidations

**Evidence**: Multiple test suites disabled:
- `crates/risk/src/engine/tests.rs:2979` - `// TODO: Re-enable once the emulator component is integrated`
- `crates/risk/src/engine/tests.rs:3074`
- `crates/risk/src/engine/tests.rs:3299`

**Acceptance Criteria**:
- [ ] Route emulated order submissions through RiskEngine first
- [ ] Re-validate orders on trigger before release to ExecEngine
- [ ] Implement pre-release validation in `fill_limit_order()` and `fill_market_order()`
- [ ] Re-enable all disabled test suites
- [ ] Document integration architecture

**Estimated Effort**: 7-10 days

---

### Issue #4: Database Cache Critical Operations Are No-Ops

**Title**: `[CRITICAL] Database cache operations implemented as no-ops`

**Labels**: `critical`, `database`, `cache`, `redis`, `bug`

**Description**:
Several critical database cache operations are implemented as no-ops or `todo!()` panics, causing data loss and incorrect state after restarts.

**Affected Methods**:

| File | Line | Method | Status |
|------|------|--------|--------|
| `nautilus_trader/cache/database.pyx` | 869-890 | `delete_account_event` | NO-OP (warns) |
| `crates/infrastructure/src/redis/cache.rs` | 387-394 | `delete_account_event` | NO-OP (warns) |
| `crates/infrastructure/src/redis/cache.rs` | 912-914 | `load_index_order_position` | `todo!()` PANIC |
| `crates/infrastructure/src/redis/cache.rs` | 916-918 | `load_index_order_client` | `todo!()` PANIC |
| `crates/infrastructure/src/redis/cache.rs` | 1087-1089 | `delete_account_event` (trait) | `todo!()` PANIC |

**Impact**:
- Account event deletion broken
- Position mappings lost after cache reload
- Order-to-client indices cannot be restored
- Risk controls may fail due to missing position data
- Strategy recovery from persistence compromised

**Acceptance Criteria**:
- [ ] Implement `delete_account_event` with proper Redis operations
- [ ] Implement `load_index_order_position` to restore order-position mappings
- [ ] Implement `load_index_order_client` to restore order-client mappings
- [ ] Add migration strategy for existing data structures
- [ ] Add comprehensive persistence tests

**Estimated Effort**: 5-7 days

---

### Issue #5: Real-time Account Balance Tracking Incomplete

**Title**: `[CRITICAL] Real-time account balance tracking not fully implemented`

**Labels**: `critical`, `portfolio`, `risk-engine`, `bug`

**Description**:
Position sizing and P&L calculations may be inaccurate due to incomplete real-time balance tracking implementation.

**Location**:
- File: `crates/risk/src/engine/tests.rs`
- Line: 3455

**Evidence**:
```rust
// TODO: Re-enable once real-time account balance tracking is implemented.
```

**Impact**:
- Position sizing calculations may be wrong
- P&L tracking may be inaccurate
- Risk limits may not reflect current state
- Balance-based risk checks unreliable

**Acceptance Criteria**:
- [ ] Implement real-time balance updates from execution events
- [ ] Ensure balance reflects pending orders
- [ ] Add balance reconciliation with exchange
- [ ] Re-enable disabled tests

**Estimated Effort**: 5-7 days

---

## P1 - High Priority Issues

### Issue #6: Complete dYdX v4 Adapter gRPC Implementation

**Title**: `[HIGH] dYdX v4 adapter has 14+ stubbed gRPC methods`

**Labels**: `enhancement`, `adapter`, `dydx`, `grpc`

**Description**:
The dYdX v4 adapter has comprehensive architecture but 14+ methods are stubbed awaiting proto file generation. The real gRPC client implementation exists but is disabled.

**Stubbed Methods in `execution/submitter.rs`**:

| Line | Method | Purpose |
|------|--------|---------|
| 75-91 | `submit_market_order` | Submit market orders via gRPC |
| 101-126 | `submit_limit_order` | Submit limit orders via gRPC |
| 133-142 | `cancel_order` | Cancel single order |
| 151-165 | `cancel_orders_batch` | Cancel multiple orders |
| 284-320 | `extract_market_params` | Get market parameters from dYdX |
| 324-327 | `handle_exchange_response` | Parse gRPC response |
| 331-334 | `parse_venue_order_id` | Extract venue order ID |
| 338-342 | `store_order_id_mapping` | Persist order ID mapping |
| 346-349 | `get_venue_order_id` | Retrieve order ID mapping |
| 353-357 | `generate_order_accepted` | Emit OrderAccepted event |
| 361-365 | `generate_order_rejected` | Emit OrderRejected event |

**Disabled Modules** (`grpc/mod.rs` lines 39-53):
```rust
// pub mod builder;      // ORDER TRANSACTION BUILDER
// pub mod client;       // FULL GRPC CLIENT
// pub mod order;        // ORDER TYPES AND QUANTIZATION
```

**Acceptance Criteria**:
- [ ] Generate proto files from dYdX v4 definitions
- [ ] Enable builder, client, and order modules
- [ ] Implement all stubbed methods with real gRPC calls
- [ ] Add integration tests against dYdX testnet
- [ ] Fix 18 ignored WebSocket/HTTP tests

**Estimated Effort**: 8-10 days

---

### Issue #7: Implement Kernel Lifecycle Management

**Title**: `[HIGH] Kernel lifecycle methods (reset/dispose/start/stop) incomplete`

**Labels**: `enhancement`, `system`, `kernel`

**Description**:
The system kernel lacks proper lifecycle management for graceful restarts and shutdowns.

**Missing Functionality**:
- Engine reset without full restart
- Proper dispose/cleanup sequences
- Start/stop state machine
- Connection timeout handling

**Acceptance Criteria**:
- [ ] Implement `reset()` for all engines
- [ ] Implement `dispose()` with proper cleanup
- [ ] Add start/stop state machine
- [ ] Handle connection timeouts gracefully
- [ ] Add lifecycle event notifications

**Estimated Effort**: 5-7 days

---

### Issue #8: Data Engine Synthetic Instrument Support

**Title**: `[HIGH] Synthetic instrument support not implemented in data engine`

**Labels**: `enhancement`, `data-engine`, `feature`

**Description**:
Multiple TODOs indicate synthetic instruments (spreads, baskets, indices) are planned but not implemented.

**Impact**:
- Cannot create spread strategies
- No basket trading support
- Custom index instruments not available

**Acceptance Criteria**:
- [ ] Design synthetic instrument data model
- [ ] Implement synthetic price calculation
- [ ] Add bar aggregation for synthetics
- [ ] Support in backtesting engine
- [ ] Add example strategies

**Estimated Effort**: 7-10 days

---

### Issue #9: Live Trading Execution Reconciliation

**Title**: `[HIGH] Live execution reconciliation incomplete`

**Labels**: `enhancement`, `live-trading`, `execution`

**Description**:
Reconciliation between local state and exchange state needs completion for reliable live trading.

**Acceptance Criteria**:
- [ ] Implement order state reconciliation on reconnect
- [ ] Add position reconciliation with exchange
- [ ] Handle partial fills during disconnect
- [ ] Add reconciliation events for monitoring

**Estimated Effort**: 7-10 days

---

### Issue #10: Fix Disabled dYdX Tests

**Title**: `[HIGH] 18 dYdX adapter tests are ignored due to flaky timing`

**Labels**: `testing`, `adapter`, `dydx`, `tech-debt`

**Description**:
18 tests in the dYdX adapter are ignored due to async timing issues with mock servers.

**Ignored Tests**:
- 14 WebSocket tests (`tests/websocket.rs`)
- 3 HTTP tests (`tests/http.rs`)
- 1 Data test (`src/data/mod.rs`)

**Root Cause**: Async operations with non-deterministic timing; mock server doesn't reliably track subscription state changes.

**Acceptance Criteria**:
- [ ] Replace mock server with deterministic test fixtures
- [ ] Use countdown latches instead of sleep timeouts
- [ ] Mock WebSocket messages directly
- [ ] Re-enable all 18 tests

**Estimated Effort**: 3-5 days

---

## P2 - Medium Priority Issues

### Issue #11: Complete Hyperliquid Adapter

**Title**: `[MEDIUM] Hyperliquid adapter marked as "Building"`

**Labels**: `enhancement`, `adapter`, `hyperliquid`

**Description**:
The Hyperliquid adapter is marked as "Building" status and needs completion.

**Acceptance Criteria**:
- [ ] Complete execution client implementation
- [ ] Add comprehensive tests
- [ ] Update documentation
- [ ] Mark as "Stable"

**Estimated Effort**: 5-7 days

---

### Issue #12: Complete Kraken Adapter

**Title**: `[MEDIUM] Kraken adapter marked as "Building"`

**Labels**: `enhancement`, `adapter`, `kraken`

**Description**:
The Kraken adapter is marked as "Building" status and needs completion.

**Acceptance Criteria**:
- [ ] Complete data and execution clients
- [ ] Add comprehensive tests
- [ ] Update documentation
- [ ] Mark as "Stable"

**Estimated Effort**: 5-7 days

---

### Issue #13: Add Actor/Strategy Persistence Methods

**Title**: `[MEDIUM] Actor and strategy persistence methods are todo!()`

**Labels**: `enhancement`, `persistence`, `cache`

**Description**:
Actor and strategy load/delete methods in Redis cache are implemented as `todo!()`.

**Location**: `crates/infrastructure/src/redis/cache.rs`
- Lines 989-991: `load_actor()`
- Lines 993-995: `delete_actor()`
- Lines 997-999: `load_strategy()`
- Lines 1001-1003: `delete_strategy()`

**Acceptance Criteria**:
- [ ] Implement actor state persistence
- [ ] Implement strategy state persistence
- [ ] Add recovery tests

**Estimated Effort**: 3-5 days

---

### Issue #14: Implement Conditional Order Types in dYdX

**Title**: `[MEDIUM] dYdX conditional orders (stop/take-profit) return NotImplemented`

**Labels**: `enhancement`, `adapter`, `dydx`

**Description**:
Several conditional order types in dYdX adapter return `DydxError::NotImplemented`.

**Affected Methods** (`execution/submitter.rs`):
- `submit_stop_market_order()` (lines 173-187)
- `submit_stop_limit_order()` (lines 195-212)
- `submit_take_profit_market_order()` (lines 220-234)
- `submit_take_profit_limit_order()` (lines 242-259)

**Acceptance Criteria**:
- [ ] Implement stop market orders
- [ ] Implement stop limit orders
- [ ] Implement take profit orders
- [ ] Add integration tests

**Estimated Effort**: 5-7 days

---

### Issue #15: Risk Engine Configuration Enhancements

**Title**: `[MEDIUM] Add margin-specific configuration to RiskEngineConfig`

**Labels**: `enhancement`, `risk-engine`, `configuration`

**Description**:
The `RiskEngineConfig` lacks margin-specific settings needed for proper margin account risk management.

**Missing Configuration**:
- Per-account risk limits
- Margin-specific thresholds
- Leverage limits by instrument
- Liquidation threshold settings
- Cross-margin vs isolated margin mode
- Position concentration limits

**Acceptance Criteria**:
- [ ] Add margin configuration fields
- [ ] Implement configuration validation
- [ ] Update documentation
- [ ] Add configuration examples

**Estimated Effort**: 3-5 days

---

## P3 - Low Priority Issues

### Issue #16: Enable Rust Clippy unwrap_used/expect_used Lints

**Title**: `[LOW] Enable strict error handling lints in Rust codebase`

**Labels**: `tech-debt`, `code-quality`, `rust`

**Description**:
The workspace Cargo.toml has `unwrap_used` and `expect_used` clippy lints commented out as TODO for incremental enablement.

**Location**: Workspace `Cargo.toml`
```toml
# TODO: Enable incrementally
# unwrap_used = "warn"
# expect_used = "warn"
```

**Acceptance Criteria**:
- [ ] Enable lints in select crates
- [ ] Replace unwrap/expect with proper error handling
- [ ] Expand to all crates incrementally

**Estimated Effort**: Ongoing (1-2 days per crate)

---

### Issue #17: Add Video Tutorials for Complex Workflows

**Title**: `[LOW] Create video tutorials for complex trading workflows`

**Labels**: `documentation`, `enhancement`

**Description**:
The documentation could benefit from video tutorials for complex workflows like multi-venue setup, strategy deployment, and backtesting.

**Suggested Topics**:
- Getting started walkthrough
- Creating your first strategy
- Running a backtest
- Deploying to live trading
- Multi-venue configuration

**Estimated Effort**: 2-3 days per video

---

### Issue #18: Improve API Reference Documentation

**Title**: `[LOW] Add narrative explanations to API reference docs`

**Labels**: `documentation`, `enhancement`

**Description**:
The API reference documentation is auto-generated and could benefit from more narrative explanations and usage examples.

**Acceptance Criteria**:
- [ ] Add usage examples to key classes
- [ ] Add cross-references between related concepts
- [ ] Include common patterns and anti-patterns

**Estimated Effort**: 5-7 days

---

## Summary Statistics

| Priority | Count | Total Effort |
|----------|-------|--------------|
| P0 Critical | 5 | 25-36 days |
| P1 High | 5 | 30-41 days |
| P2 Medium | 5 | 21-31 days |
| P3 Low | 3 | 8-12 days |
| **Total** | **18** | **84-120 days** |

---

## Recommended Implementation Order

### Phase 1: Critical Stability (Weeks 1-3)
1. Issue #1: Risk Engine Multi-Venue Bypass
2. Issue #2: Margin Account Controls
3. Issue #3: Order Emulator Risk Integration
4. Issue #4: Database Cache Operations
5. Issue #5: Real-time Balance Tracking

### Phase 2: High Priority Features (Weeks 4-6)
6. Issue #6: dYdX gRPC Implementation
7. Issue #7: Kernel Lifecycle Management
8. Issue #10: Fix Disabled dYdX Tests

### Phase 3: Exchange Expansion (Weeks 7-9)
9. Issue #11: Hyperliquid Adapter
10. Issue #12: Kraken Adapter
11. Issue #14: dYdX Conditional Orders

### Phase 4: Polish & Quality (Weeks 10-12)
12. Issue #8: Synthetic Instruments
13. Issue #9: Execution Reconciliation
14. Issue #13: Actor/Strategy Persistence
15. Issue #15: Risk Configuration

### Ongoing
16. Issue #16: Clippy Lints
17. Issue #17: Video Tutorials
18. Issue #18: API Documentation

---

*This document should be used as a template for creating actual GitHub issues. Each issue contains sufficient detail for implementation.*
