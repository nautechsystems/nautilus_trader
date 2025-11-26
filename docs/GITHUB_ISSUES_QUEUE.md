# GitHub Issues Queue - NautilusTrader

Generated: 2025-11-26

---

## CRITICAL (P0) - Create Immediately

### Issue #1: Risk Engine Multi-Venue Support
**Labels:** `critical`, `risk-engine`, `multi-venue`, `security`

**Title:** `[Risk Engine] Implement proper risk controls for multiple venues routing`

**Body:**
```markdown
## Summary
Currently, the risk engine returns `true` (allowing all orders) when handling multiple venues. This is a temporary workaround that bypasses critical risk controls.

## Location
- **File:** `crates/risk/src/engine/mod.rs:627`
- **Code:** `return true; // TODO: Temporary early return until handling routing/multiple venues`

## Impact
Risk controls are completely bypassed for multi-venue scenarios, potentially allowing dangerous positions.

## Acceptance Criteria
- [ ] Implement venue-specific risk checks
- [ ] Handle routing logic for multi-venue orders
- [ ] Add comprehensive tests for cross-venue risk scenarios
- [ ] Document risk control behavior per venue

## Estimated Effort
3-5 days
```

---

### Issue #2: Risk Engine Margin Account Controls
**Labels:** `critical`, `risk-engine`, `margin-trading`, `security`

**Title:** `[Risk Engine] Implement risk controls for margin trading accounts`

**Body:**
```markdown
## Summary
Margin accounts currently have no risk controls. All orders are approved regardless of margin requirements, leverage limits, or maintenance margin.

## Location
- **File:** `crates/risk/src/engine/mod.rs:631`
- **Code:** `AccountAny::Margin(_) => return true, // TODO: Determine risk controls for margin`

## Impact
No risk controls for margin accounts - all trades approved unconditionally.

## Acceptance Criteria
- [ ] Calculate and validate margin requirements per order
- [ ] Enforce maximum leverage limits
- [ ] Check maintenance margin thresholds
- [ ] Implement margin call detection
- [ ] Add liquidation risk warnings

## Estimated Effort
5-7 days
```

---

### Issue #3: Database Cache Efficiency Reimplementation
**Labels:** `critical`, `performance`, `database`, `cache`

**Title:** `[Cache] Reimplement database cache operations for efficiency`

**Body:**
```markdown
## Summary
Database cache operations are currently implemented as no-ops pending a performance-focused reimplementation.

## Location
- **File:** `nautilus_trader/cache/database.pyx:886`
- **Code:** `# TODO: No-op pending reimplementation to improve efficiency`

## Impact
Critical database operations not persisting data; affects data recovery.

## Acceptance Criteria
- [ ] Profile current database bottlenecks
- [ ] Design efficient batch update strategy
- [ ] Implement bulk write operations
- [ ] Add database connection pooling
- [ ] Benchmark against old implementation (2x improvement target)

## Estimated Effort
5-7 days
```

---

### Issue #4: Real-time Account Balance Tracking
**Labels:** `critical`, `portfolio`, `real-time`, `accounting`

**Title:** `[Portfolio] Implement real-time account balance tracking`

**Body:**
```markdown
## Summary
Tests are disabled because real-time balance tracking is not implemented. This affects position sizing, risk calculations, and PnL reporting.

## Location
- **File:** `crates/risk/src/engine/tests.rs:3455`
- **Code:** `// TODO: Re-enable once real-time account balance tracking is implemented.`

## Impact
Position sizing and PnL calculations may be inaccurate.

## Acceptance Criteria
- [ ] Implement real-time balance updates on fills
- [ ] Track unrealized PnL continuously
- [ ] Handle multi-currency balance conversion
- [ ] Re-enable and update disabled tests
- [ ] Add balance reconciliation checks

## Estimated Effort
5-7 days
```

---

### Issue #5: Order Emulator Integration with Risk Engine
**Labels:** `critical`, `risk-engine`, `order-emulator`, `integration`

**Title:** `[Risk + Execution] Integrate Order Emulator with Risk Engine`

**Body:**
```markdown
## Summary
The order emulator (handles stop/trailing orders client-side) is not yet integrated with the risk engine. Multiple test suites are disabled.

## Locations
- `crates/risk/src/engine/tests.rs:2979`
- `crates/risk/src/engine/tests.rs:3074`
- `crates/risk/src/engine/tests.rs:3299`
- **Code:** `// TODO: Re-enable once the emulator component is integrated with the risk engine.`

## Impact
Risk checks not applied to emulated orders; tests disabled.

## Acceptance Criteria
- [ ] Design risk check workflow for emulated orders
- [ ] Implement pre-submission risk validation
- [ ] Add post-trigger risk re-validation
- [ ] Handle emulator state in risk calculations
- [ ] Re-enable all disabled test suites

## Estimated Effort
7-10 days
```

---

## HIGH PRIORITY (P1) - Create This Sprint

### Issue #6: Kernel Engine Lifecycle Management
**Labels:** `high-priority`, `system`, `lifecycle`, `engines`

**Title:** `[Kernel] Complete engine lifecycle management (reset/dispose/start/stop)`

**Body:**
```markdown
## Summary
The system kernel cannot properly manage engine lifecycles. Only some engines have reset/dispose/start/stop implemented.

## Locations
- `crates/system/src/kernel.rs:441` - Reset methods
- `crates/system/src/kernel.rs:460` - Dispose methods
- `crates/system/src/kernel.rs:475` - Start methods
- `crates/system/src/kernel.rs:481` - Stop methods

## Acceptance Criteria
- [ ] Implement lifecycle methods for Data Engine
- [ ] Implement lifecycle methods for Execution Engine
- [ ] Implement lifecycle methods for Portfolio Engine
- [ ] Add proper shutdown sequencing
- [ ] Test clean restarts and state recovery

## Estimated Effort
5-7 days
```

---

### Issue #7: Kernel Timeout Handling
**Labels:** `high-priority`, `system`, `reliability`, `timeouts`

**Title:** `[Kernel] Implement timeout handling for startup operations`

**Body:**
```markdown
## Summary
Critical kernel startup operations have no timeout handling, which can cause indefinite hangs.

## Locations
- `crates/system/src/kernel.rs:540` - Engine connections
- `crates/system/src/kernel.rs:547` - Execution reconciliation
- `crates/system/src/kernel.rs:554` - Portfolio initialization
- `crates/system/src/kernel.rs:561` - Trader residual events

## Acceptance Criteria
- [ ] Add configurable timeouts for each startup phase
- [ ] Implement graceful timeout failures
- [ ] Add retry logic for transient failures
- [ ] Log detailed timeout diagnostics
- [ ] Test timeout scenarios

## Estimated Effort
3-5 days
```

---

### Issue #8: Data Engine Synthetic Instrument Support
**Labels:** `high-priority`, `data-engine`, `synthetic-instruments`, `feature`

**Title:** `[Data Engine] Implement synthetic instrument support`

**Body:**
```markdown
## Summary
The data engine cannot handle synthetic instruments (spread, basket, etc.).

## Locations
- `crates/data/src/engine/mod.rs:780`
- `crates/data/src/engine/mod.rs:791`
- **Code:** `// TODO: Handle synthetics`

## Acceptance Criteria
- [ ] Define synthetic instrument data flow
- [ ] Implement component data aggregation
- [ ] Handle synthetic quote/trade generation
- [ ] Add synthetic orderbook support
- [ ] Test common synthetic types (spreads, baskets)

## Estimated Effort
7-10 days
```

---

### Issue #9: Live Execution Reconciliation
**Labels:** `high-priority`, `live-trading`, `execution`, `reconciliation`

**Title:** `[Live Trading] Implement execution reconciliation for missing fills`

**Body:**
```markdown
## Summary
Live execution manager cannot reconcile missing fills or create orders from exchange reports. Critical for recovering from connection drops.

## Locations
- `crates/live/src/execution/manager.rs:813`
- `crates/live/src/execution/manager.rs:847`
- **Code:** `// TODO: Query for missing fills to reconcile the discrepancy`

## Acceptance Criteria
- [ ] Implement fill reconciliation queries
- [ ] Create orders from exchange reports
- [ ] Handle partial fills during reconnection
- [ ] Add reconciliation audit logs
- [ ] Test reconnection scenarios

## Estimated Effort
7-10 days
```

---

### Issue #10: dYdX v4 Adapter Completion
**Labels:** `high-priority`, `adapter`, `dydx`, `proto`

**Title:** `[dYdX Adapter] Complete proto integration and order submission`

**Body:**
```markdown
## Summary
The dYdX v4 adapter has stub implementations that need to be replaced with actual order submission logic.

## Key Finding
**Proto files do NOT need to be generated** - the `dydx-proto` crate (v0.4.0) provides all needed definitions.

## Current Status
- ✅ Dependencies present (`dydx-proto`, gRPC, Cosmos SDK)
- ✅ gRPC client implemented (469 lines, commented out)
- ✅ OrderBuilder implemented (721 lines, commented out)
- ❌ Proto module disabled (needs 5-line re-export)
- ❌ Order submission stubbed

## Files to Modify
- `src/proto/mod.rs` - Add re-export (5 lines)
- `src/lib.rs` - Uncomment proto module (1 line)
- `src/grpc/mod.rs` - Remove stubs, enable exports (~120 lines deleted)
- `src/execution/submitter.rs` - Implement order submission (~300-400 lines)

## Acceptance Criteria
- [ ] Enable proto module with re-exports
- [ ] Remove all stub code from grpc module
- [ ] Implement market/limit order submission
- [ ] Implement order cancellation
- [ ] Implement conditional orders (stop/take-profit)
- [ ] Add testnet integration tests
- [ ] Complete documentation

## Estimated Effort
8-10 hours

## References
- `docs/dydx_proto_analysis.md` - Detailed technical analysis
- `docs/dydx_proto_remediation_plan.md` - Step-by-step implementation guide
```

---

## Statistics Summary

| Priority | Count | Estimated Days |
|----------|-------|----------------|
| Critical (P0) | 5 | 25-36 days |
| High (P1) | 5 | 27-44 days |
| Medium (P2) | 80 | ~120 days |
| Low (P3) | 60+ | ~60 days |
| **Total** | **150+** | **~230 days** |

---

## Component Breakdown

| Component | Critical | High | Medium | Low |
|-----------|----------|------|--------|-----|
| Risk Engine | 5 | 3 | 8 | 2 |
| System/Kernel | 3 | 5 | 2 | 1 |
| Data Engine | 1 | 6 | 12 | 3 |
| Execution | 2 | 4 | 8 | 2 |
| Portfolio | 1 | 3 | 5 | 2 |
| Adapters | 0 | 8 | 15 | 10 |
| Testing | 0 | 2 | 10 | 5 |

---

## How to Create These Issues

### Using GitHub CLI (if installed):
```bash
gh issue create --title "[Risk Engine] Implement proper risk controls for multiple venues routing" \
  --body "$(cat issue_body.md)" \
  --label "critical,risk-engine,multi-venue,security"
```

### Using GitHub Web Interface:
1. Go to repository Issues tab
2. Click "New Issue"
3. Copy title and body from above
4. Add appropriate labels
5. Submit

---

## Next Steps

1. **Immediate**: Create P0 issues (5 critical)
2. **This Sprint**: Create P1 issues (5 high priority)
3. **Backlog**: Import remaining as tracking epic
4. **Automation**: Set up TODO comment tracking in CI

