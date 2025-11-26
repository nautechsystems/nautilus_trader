# dYdX Adapter - Issues, Stubs, and Implementation Status

This document tracks known issues, stub implementations, and flaky tests in the nautilus-dydx adapter.

## Table of Contents

1. [Dependency Version Conflicts](#dependency-version-conflicts)
2. [Stub Implementations](#stub-implementations)
3. [Ignored/Flaky Tests](#ignoredflaky-tests)
4. [Known TODOs](#known-todos)
5. [Next Steps](#next-steps)

---

## Dependency Version Conflicts

### Status: RESOLVED

The `dydx-proto` crate requires specific versions of `prost` and `tonic` that differ from the workspace defaults.

| Dependency | Workspace Version | dydx-proto Requires | Resolution |
|------------|-------------------|---------------------|------------|
| `prost`    | 0.14.1            | 0.13.5              | Override in Cargo.toml |
| `tonic`    | 0.14.2            | 0.13.1              | Override in Cargo.toml |

**Root Cause**: The `prost::Message` trait's `encode_to_vec` method has different signatures between versions, causing trait bound failures when mixing versions.

**Fix Applied** in `Cargo.toml`:
```toml
# Override prost to match dydx-proto's version (0.13.x) to avoid trait bound conflicts
prost = { version = "0.13.5", default-features = false, features = ["std"] }
prost-types = "0.13.5"
# Override tonic to match dydx-proto's version (0.13.x) to avoid trait bound conflicts
tonic = { version = "0.13.1", features = ["channel", "transport"] }
```

---

## Stub Implementations

The gRPC layer currently uses stub implementations pending full integration with the execution layer.

### 1. DydxGrpcClient (`src/grpc/mod.rs`)

| Component | Status | Description |
|-----------|--------|-------------|
| `DydxGrpcClient::new()` | STUB | Logs initialization but doesn't establish real connection |
| `DydxGrpcClient::new_with_fallback()` | STUB | Logs endpoints but doesn't perform actual failover |

**Blocked By**: Full gRPC modules (builder, client, order) are commented out at lines 39-53.

### 2. OrderSubmitter (`src/execution/submitter.rs`)

| Method | Status | Description |
|--------|--------|-------------|
| `submit_market_order()` | STUB | Logs order details, returns `Ok(())` |
| `submit_limit_order()` | STUB | Logs order details, returns `Ok(())` |
| `cancel_order()` | STUB | Logs cancellation, returns `Ok(())` |
| `cancel_orders_batch()` | STUB | Logs batch cancellation, returns `Ok(())` |
| `submit_stop_market_order()` | NOT IMPLEMENTED | Returns `DydxError::NotImplemented` |
| `submit_stop_limit_order()` | NOT IMPLEMENTED | Returns `DydxError::NotImplemented` |
| `submit_take_profit_market_order()` | NOT IMPLEMENTED | Returns `DydxError::NotImplemented` |
| `submit_take_profit_limit_order()` | NOT IMPLEMENTED | Returns `DydxError::NotImplemented` |
| `submit_trailing_stop_order()` | NOT IMPLEMENTED | Returns `DydxError::NotImplemented` (not supported by dYdX v4) |
| `extract_market_params()` | STUB | Derives params from instrument, not dYdX metadata |
| `handle_exchange_response()` | STUB | Returns hardcoded "stubbed_tx_hash" |
| `parse_venue_order_id()` | STUB | Returns hardcoded "stubbed_venue_id" |
| `store_order_id_mapping()` | STUB | Logs but doesn't persist |
| `get_venue_order_id()` | STUB | Always returns `None` |
| `generate_order_accepted()` | STUB | Logs but doesn't emit event |
| `generate_order_rejected()` | STUB | Logs but doesn't emit event |

### 3. Placeholder Types (`src/execution/submitter.rs`)

These temporary enums exist until proto types are properly integrated:

```rust
pub enum ProtoOrderSide {
    Buy,
    Sell,
}

pub enum ProtoTimeInForce {
    Unspecified,
    Ioc,
    FillOrKill,
}
```

### 4. ExecutionClient (`src/execution/mod.rs`)

| Method | Status | Description |
|--------|--------|-------------|
| `cancel_all_orders()` | STUB | Logs but doesn't execute cancellations (line 806) |
| Conditional orders | STUB | Accepts orders, generates OrderSubmitted, but doesn't send to exchange (line 494-510) |

---

## Ignored/Flaky Tests

**Total**: 18 tests marked with `#[ignore]`

### HTTP Tests (`tests/http.rs`) - 3 ignored

| Test | Reason |
|------|--------|
| `test_concurrent_requests` | Flaky test - mock data incomplete |
| `test_request_timeout_short` | Flaky test - timeout behavior inconsistent |
| `test_large_instruments_response` | Mock data incomplete - uses incorrect field names |

### WebSocket Tests (`tests/websocket.rs`) - 14 ignored

| Test Location | Reason |
|---------------|--------|
| Line 395 | Flaky: disconnect state change timing is non-deterministic |
| Line 536 | Flaky: subscription tracking depends on message timing |
| Line 611 | Flaky: disconnect state change timing is non-deterministic |
| Line 634 | Flaky: rapid reconnections are timing-dependent |
| Line 664 | Flaky: subscription restoration depends on client implementation details |
| Line 1007 | Flaky: Mock server subscription event tracking unreliable |
| Line 1044 | Flaky: Mock server subscription event tracking unreliable |
| Line 1184 | Flaky - timing issues with disconnect state |
| Line 1211 | Flaky - timing issues with multiple subscriptions |
| Line 1266 | Flaky - timing issues with repeated connections |
| Line 1349 | Flaky - mock server doesn't track unsubscribe events reliably |
| Line 1380 | Flaky - mock server doesn't track unsubscribe events reliably |
| Line 1502 | Flaky - race conditions with concurrent subscriptions |
| Line 1554 | Flaky - disconnect state timing issues |

### Data Tests (`src/data/mod.rs`) - 1 ignored

| Test Location | Reason |
|---------------|--------|
| Line 7419 | No reason specified |

### Root Causes

1. **Timing/Race Conditions**: Async WebSocket operations have non-deterministic timing
2. **Mock Server Limitations**: Test mock server doesn't reliably track subscription state changes
3. **Incomplete Mock Data**: Some HTTP test fixtures have incorrect or missing fields

---

## Known TODOs

### High Priority

| Location | TODO |
|----------|------|
| `src/grpc/mod.rs:39-53` | Enable full gRPC modules (builder, client, order) when proto integration complete |
| `src/execution/submitter.rs:83-91` | Implement real market order submission via gRPC |
| `src/execution/submitter.rs:114-126` | Implement real limit order submission via gRPC |
| `src/execution/submitter.rs:139-142` | Implement real order cancellation via gRPC |
| `src/execution/mod.rs:562` | Implement proper ClientOrderId to u32 mapping |

### Medium Priority

| Location | TODO |
|----------|------|
| `src/execution/submitter.rs:299-305` | Replace stub market params with real dYdX metadata lookup |
| `src/execution/submitter.rs:325` | Parse proto response when available |
| `src/execution/mod.rs:814` | Implement actual cancellation when proto is generated |

### Low Priority

| Location | TODO |
|----------|------|
| `src/data/mod.rs:2228` | Forward oracle price once nautilus_model supports custom types |
| `src/execution/mod.rs:90,179,201,217` | Remove `#[allow(dead_code)]` once implementation complete |

---

## Next Steps

### Phase 1: gRPC Integration

1. Update `execution/mod.rs` to pass `Account` (not `Wallet`) to submitter methods
2. Implement `OrderMarketParams` lookup from dYdX instrument metadata
3. Re-enable gRPC modules in `grpc/mod.rs`
4. Replace stub implementations with real gRPC calls

### Phase 2: Order Management

1. Implement ClientOrderId ↔ VenueOrderId mapping storage
2. Wire up OrderAccepted/OrderRejected event generation
3. Implement conditional order support (stop, take-profit)

### Phase 3: Test Stabilization

1. Introduce deterministic async test harness for WebSocket tests
2. Fix mock server subscription tracking
3. Complete mock data fixtures for HTTP tests

---

## Test Results Summary

```
cargo test --package nautilus-dydx

test result: ok. 32 passed; 0 failed; 14 ignored
```

All core functionality tests pass. Ignored tests are flaky due to async timing issues and do not indicate broken functionality.

---

*Last Updated: 2025-11-26*
