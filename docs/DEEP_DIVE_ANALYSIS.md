# Deep Dive Analysis: Critical Components

**Generated**: November 27, 2025
**Purpose**: Detailed technical analysis for implementation planning

---

## Table of Contents

1. [Risk Engine Deep Dive](#1-risk-engine-deep-dive)
2. [Order Emulator Deep Dive](#2-order-emulator-deep-dive)
3. [Database Cache Deep Dive](#3-database-cache-deep-dive)
4. [dYdX Adapter Deep Dive](#4-dydx-adapter-deep-dive)

---

## 1. Risk Engine Deep Dive

### 1.1 Architecture Overview

The risk engine is implemented in Rust at `crates/risk/src/engine/mod.rs` (1,187 lines).

**Key Functions**:
- `check_order()` - Basic order validation (price, quantity, expiry)
- `check_orders_risk()` - Comprehensive risk checks (notional, balance, margin)
- `handle_submit_order()` - Entry point for order submission

### 1.2 Critical Gap #1: Multi-Venue Bypass

**Location**: `crates/risk/src/engine/mod.rs:617-628`

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

**Problem**: When trading across multiple venues (e.g., Binance + Bybit), if the specific venue account isn't cached, ALL risk checks are bypassed.

**Root Cause**: Single-venue lookup assumes 1:1 mapping between venue and account.

**Solution Approach**:
```rust
// Proposed implementation
let account_exists = {
    let cache = self.cache.borrow();

    // First try venue-specific account
    if let Some(acc) = cache.account_for_venue(&instrument.id().venue).cloned() {
        Some(acc)
    } else {
        // Fallback: aggregate accounts or use default
        let strategy_account = cache.account_for_strategy(&self.strategy_id);
        strategy_account.or_else(|| cache.accounts().first().cloned())
    }
};

let account = match account_exists {
    Some(acc) => acc,
    None => {
        log::error!("No valid account found for order - DENYING");
        return false; // DENY instead of ALLOW
    }
};
```

### 1.3 Critical Gap #2: Margin Account Bypass

**Location**: `crates/risk/src/engine/mod.rs:629-632`

```rust
let cash_account = match account {
    AccountAny::Cash(cash_account) => cash_account,
    AccountAny::Margin(_) => return true, // TODO: Determine risk controls for margin
};
```

**Problem**: All ~400 lines of balance checking code (lines 633-1027) are skipped for margin accounts.

**Missing Controls**:
| Control | Method Available | Used? |
|---------|-----------------|-------|
| Leverage limits | `MarginAccount::get_leverage()` | ❌ |
| Initial margin | `MarginAccount::initial_margins()` | ❌ |
| Maintenance margin | `MarginAccount::maintenance_margins()` | ❌ |
| Free margin | `MarginAccount::get_margin_balance()` | ❌ |

**Solution Approach**:
```rust
fn check_margin_account_risk(
    &self,
    margin_account: &MarginAccount,
    instrument: &InstrumentAny,
    orders: &[OrderAny],
) -> bool {
    // 1. Get current leverage for instrument
    let leverage = margin_account.get_leverage(&instrument.id())
        .unwrap_or(margin_account.default_leverage);

    // 2. Check against max allowed leverage
    if let Some(max_leverage) = self.config.max_leverage.get(&instrument.id()) {
        if leverage > *max_leverage {
            log::warn!("Leverage {} exceeds max {}", leverage, max_leverage);
            return false;
        }
    }

    // 3. Calculate order notional
    let notional = calculate_order_notional(orders, instrument);

    // 4. Check initial margin requirement
    let required_margin = notional / leverage;
    let free_margin = margin_account.get_margin_balance(instrument.quote_currency())
        .map(|b| b.free)
        .unwrap_or(Decimal::ZERO);

    if required_margin > free_margin {
        log::warn!("Insufficient margin: required {} > free {}", required_margin, free_margin);
        return false;
    }

    // 5. Check maintenance margin buffer
    let maintenance = margin_account.maintenance_margins()
        .get(&instrument.id())
        .cloned()
        .unwrap_or(Decimal::ZERO);

    let post_trade_margin = free_margin - required_margin;
    if post_trade_margin < maintenance {
        log::warn!("Would breach maintenance margin");
        return false;
    }

    true
}
```

### 1.4 Test Coverage Analysis

**Current Test Count**: ~60 tests in `crates/risk/src/engine/tests.rs`

| Category | Tests | Status |
|----------|-------|--------|
| Price/Quantity validation | ~15 | ✅ Complete |
| Notional limits | ~10 | ✅ Complete |
| Free balance checks | ~8 | ✅ Complete |
| Reduce-only orders | ~5 | ✅ Complete |
| Trading state | ~6 | ✅ Complete |
| **Margin account risk** | 1 | ❌ BROKEN |
| **Multi-venue routing** | 0 | ❌ MISSING |

**Key Disabled Tests**:
- Line 3306: `test_submit_order_when_market_order_and_over_free_balance_then_denies_with_betting_account`
- Lines 2979, 3074, 3299: Emulator integration tests

---

## 2. Order Emulator Deep Dive

### 2.1 Architecture Overview

**Files**:
- `crates/execution/src/order_emulator/emulator.rs` (1,187 lines)
- `crates/execution/src/order_emulator/handlers.rs` (94 lines)
- `crates/execution/src/order_emulator/adapter.rs` (95 lines)

### 2.2 Emulated Order Types

| Order Type | Triggered Becomes | Supported |
|------------|------------------|-----------|
| StopMarket | Market | ✅ |
| StopLimit | Limit | ✅ |
| MarketIfTouched | Market | ✅ |
| LimitIfTouched | Limit | ✅ |
| TrailingStopMarket | Market | ✅ |
| TrailingStopLimit | Limit | ✅ |

### 2.3 Order Flow Analysis

**Current Flow** (PROBLEMATIC):

```
1. Strategy.submit_order(StopLimit)
   ↓
2. OrderEmulator.handle_submit_order() [Line 276]
   ├─ NO risk validation performed
   ├─ Order cached in OrderManager
   ├─ Subscribed to market data
   └─ OrderEmulated event sent to RiskEngine.process (monitoring only)
   ↓
3. Market data update triggers order [Line 822]
   ├─ trigger_stop_order() called
   └─ Order transformed to Limit/Market
   ↓
4. fill_limit_order() [Line 837] or fill_market_order() [Line 990]
   ├─ Creates new order with transformed type
   └─ self.manager.send_exec_command() [Line 982]
   ↓
5. ExecEngine receives order DIRECTLY
   └─ RiskEngine COMPLETELY BYPASSED
```

**Critical Code** (`emulator.rs:978-983`):
```rust
if let Some(exec_algorithm_id) = order.exec_algorithm_id() {
    self.manager.send_algo_command(command, exec_algorithm_id);
} else {
    self.manager.send_exec_command(TradingCommand::SubmitOrder(command));
    // ^^^ Direct to ExecEngine, NOT through RiskEngine!
}
```

### 2.4 Risk Bypass Evidence

**Manager routing** (`order_manager.rs:549-581`):
```rust
pub fn send_risk_command(&self, command: TradingCommand) {
    msgbus::send_any("RiskEngine.execute".into(), &command);  // Risk checks here
}

pub fn send_exec_command(&self, command: TradingCommand) {
    msgbus::send_any("ExecEngine.execute".into(), &command);  // NO risk checks
}
```

The emulator uses `send_exec_command()` not `send_risk_command()`.

### 2.5 Solution Architecture

**Proposed Flow**:

```
1. Strategy.submit_order(StopLimit)
   ↓
2. RiskEngine.handle_submit_order() [FIRST]
   ├─ check_order() - basic validation
   ├─ check_orders_risk() - notional/balance
   └─ If passed: route to OrderEmulator
   ↓
3. OrderEmulator.handle_submit_order()
   ├─ Order cached in OrderManager
   ├─ Subscribed to market data
   └─ Held until trigger
   ↓
4. Market data triggers order
   ↓
5. OrderEmulator.pre_release_validation() [NEW]
   ├─ Re-check balance availability
   ├─ Re-check notional limits
   └─ Verify trading state allows order
   ↓
6. RiskEngine.handle_emulated_release() [NEW]
   ├─ Final risk validation
   └─ If passed: route to ExecEngine
   ↓
7. ExecEngine receives validated order
```

---

## 3. Database Cache Deep Dive

### 3.1 Architecture Overview

```
┌────────────────────────────────────────┐
│  Python/Cython Layer                   │
│  nautilus_trader/cache/database.pyx    │
│  - CacheDatabaseAdapter class          │
│  - Delegates to Rust via PyO3          │
└──────────────┬─────────────────────────┘
               │ PyO3 Bindings
               ↓
┌────────────────────────────────────────┐
│  Rust Layer                            │
│  crates/infrastructure/src/redis/      │
│  - RedisCacheDatabase (pub interface)  │
│  - CacheDatabaseAdapter trait impl     │
└──────────────┬─────────────────────────┘
               │ Async Channels
               ↓
┌────────────────────────────────────────┐
│  Redis Backend                         │
│  - Lists: accounts:{id}                │
│  - Hashes: index:order_position        │
│  - Sets: index:orders                  │
└────────────────────────────────────────┘
```

### 3.2 No-Op Methods

**Python Layer** (`cache/database.pyx:869-890`):
```cython
def delete_account_event(self, AccountId account_id, str event_id):
    Condition.not_none(account_id, "account_id")
    Condition.not_none(event_id, "event_id")

    self._log.warning(f"Deleting account events currently a no-op (pending redesign)")

    # TODO: No-op pending reimplementation
    # self._backing.delete_account_event(account_id.to_str(), event_id)
```

**Rust Layer** (`redis/cache.rs:387-394`):
```rust
pub fn delete_account_event(
    &self,
    _account_id: &AccountId,  // underscore = unused
    _event_id: &str,
) -> anyhow::Result<()> {
    tracing::warn!("Deleting account events currently a no-op (pending redesign)");
    Ok(())
}
```

### 3.3 TODO Panic Methods

| Line | Method | Impact |
|------|--------|--------|
| 912-914 | `load_index_order_position()` | Position recovery broken |
| 916-918 | `load_index_order_client()` | Order routing broken |
| 989-991 | `load_actor()` | Actor state recovery broken |
| 993-995 | `delete_actor()` | Actor cleanup broken |
| 997-999 | `load_strategy()` | Strategy state recovery broken |
| 1001-1003 | `delete_strategy()` | Strategy cleanup broken |

### 3.4 Implementation Requirements

**For `load_index_order_position`**:
```rust
fn load_index_order_position(&self) -> anyhow::Result<AHashMap<ClientOrderId, Position>> {
    let mut conn = self.pool.get()?;
    let data: HashMap<String, String> = conn.hgetall("index:order_position")?;

    let mut result = AHashMap::new();
    for (order_id_str, position_json) in data {
        let order_id = ClientOrderId::new(&order_id_str);
        let position: Position = serde_json::from_str(&position_json)?;
        result.insert(order_id, position);
    }
    Ok(result)
}
```

---

## 4. dYdX Adapter Deep Dive

### 4.1 Implementation Status

| Component | File | Status |
|-----------|------|--------|
| HTTP Client | `http/client.rs` | ✅ Complete |
| WebSocket Client | `websocket/client.rs` | ✅ Complete |
| gRPC Stub | `grpc/mod.rs` | ⚠️ STUB |
| gRPC Client (Real) | `grpc/client.rs` | ✅ Complete but DISABLED |
| Order Builder | `grpc/order.rs` | ✅ Complete but DISABLED |
| Tx Builder | `grpc/builder.rs` | ✅ Complete but DISABLED |
| Execution Client | `execution/mod.rs` | ⚠️ Partial |
| Order Submitter | `execution/submitter.rs` | ⚠️ STUB |

### 4.2 Stubbed Methods in `execution/submitter.rs`

```rust
// Line 75-91
async fn submit_market_order(&self, ...) -> Result<(), DydxError> {
    tracing::info!("[STUB] submit_market_order: client_order_id={}", client_order_id);
    // TODO: Implement when proto is generated
    Ok(())
}

// Line 101-126
async fn submit_limit_order(&self, ...) -> Result<(), DydxError> {
    tracing::info!("[STUB] submit_limit_order: client_order_id={}", client_order_id);
    // TODO: Implement when proto is generated
    Ok(())
}

// Line 133-142
async fn cancel_order(&self, ...) -> Result<(), DydxError> {
    tracing::info!("[STUB] cancel_order: client_order_id={}", client_order_id);
    // TODO: Implement when proto is generated
    Ok(())
}
```

### 4.3 Disabled Modules

**File**: `grpc/mod.rs:39-53`
```rust
// These are COMPLETE implementations but DISABLED:
// pub mod builder;      // TxBuilder - transaction signing
// pub mod client;       // DydxGrpcClient - full gRPC client
// pub mod order;        // OrderBuilder - order construction
```

**Why Disabled**: Waiting for proto file generation from dYdX v4 definitions.

### 4.4 Conditional Orders Status

| Order Type | Method | Status |
|------------|--------|--------|
| StopMarket | `submit_stop_market_order()` | Returns `NotImplemented` |
| StopLimit | `submit_stop_limit_order()` | Returns `NotImplemented` |
| TakeProfitMarket | `submit_take_profit_market_order()` | Returns `NotImplemented` |
| TakeProfitLimit | `submit_take_profit_limit_order()` | Returns `NotImplemented` |
| TrailingStop | `submit_trailing_stop_order()` | Not supported by dYdX v4 |

### 4.5 Test Status

**Ignored Tests**: 18 total

| Category | Count | Reason |
|----------|-------|--------|
| WebSocket | 14 | Flaky timing with mock server |
| HTTP | 3 | Mock data incomplete |
| Data | 1 | Not specified |

### 4.6 Implementation Path

1. **Generate Proto Files**
   - Clone dYdX v4 proto definitions
   - Run `prost-build` to generate Rust types
   - Add generated code to `grpc/` module

2. **Enable Disabled Modules**
   - Uncomment `pub mod builder;`
   - Uncomment `pub mod client;`
   - Uncomment `pub mod order;`

3. **Implement Stubbed Methods**
   - Replace logging with actual gRPC calls
   - Use OrderBuilder for order construction
   - Use TxBuilder for transaction signing
   - Use DydxGrpcClient for broadcasting

4. **Fix Tests**
   - Replace mock server with deterministic fixtures
   - Use countdown latches instead of timeouts
   - Re-enable all 18 ignored tests

---

## Summary: Implementation Priority Matrix

| Component | Severity | Effort | Dependencies | Priority |
|-----------|----------|--------|--------------|----------|
| Risk: Multi-Venue | CRITICAL | 3-5d | None | 1 |
| Risk: Margin | CRITICAL | 5-7d | None | 2 |
| Order Emulator Integration | CRITICAL | 7-10d | Risk Engine | 3 |
| Database Cache | CRITICAL | 5-7d | None | 4 |
| Balance Tracking | CRITICAL | 5-7d | Database Cache | 5 |
| dYdX gRPC | HIGH | 8-10d | Proto files | 6 |
| Kernel Lifecycle | HIGH | 5-7d | None | 7 |
| dYdX Tests | HIGH | 3-5d | None | 8 |

**Total Critical Path**: ~35-48 days for P0 issues
**Total High Priority**: ~16-22 days for P1 issues

---

*This deep dive document provides implementation-ready technical specifications for the critical components identified in the state of the art analysis.*
