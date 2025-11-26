# dYdX v4 Adapter Proto Integration - Remediation Plan

## Overview

This document provides a step-by-step action plan to complete the dYdX v4 adapter proto integration. The work involves enabling existing proto definitions from the `dydx-proto` crate rather than generating new ones.

## Prerequisites

- [x] `dydx-proto = "0.4.0"` dependency already in Cargo.toml
- [x] All required Cosmos SDK and gRPC dependencies present
- [x] gRPC client implementation exists in `src/grpc/client.rs`
- [x] Order builder implementation exists in `src/grpc/order.rs`
- [x] Transaction builder exists in `src/grpc/builder.rs`

## Phase 1: Enable Proto Module (30 minutes)

### Task 1.1: Create Proto Re-Export Module

**File**: `crates/adapters/dydx/src/proto/mod.rs`

**Action**: Replace current placeholder content with:

```rust
// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
//
//  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
//  You may not use this file except in compliance with the License.
//  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
//
//  Unless required by applicable law or agreed to in writing, software
//  distributed under the License is distributed on an "AS IS" BASIS,
//  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
//  See the License for the specific language governing permissions and
//  limitations under the License.
// -------------------------------------------------------------------------------------------------

//! Protocol Buffer definitions for dYdX v4.
//!
//! This module re-exports the `dydx-proto` crate which contains pre-compiled
//! Protocol Buffer definitions from the official dYdX v4 chain repository.
//!
//! The proto definitions include:
//! - Cosmos SDK base types and gRPC services
//! - dYdX protocol-specific messages (orders, subaccounts, markets, etc.)
//! - gRPC service clients for querying and broadcasting transactions

// Re-export the entire dydx-proto crate for full access
pub use dydx_proto::*;

// Convenience re-exports for commonly used modules
pub use dydx_proto::{
    cosmos_sdk_proto,
    dydxprotocol,
};
```

**Verification**:
```bash
cargo check --package nautilus-dydx
```

### Task 1.2: Enable Proto Module in Library

**File**: `crates/adapters/dydx/src/lib.rs`

**Action**: Line 60 - Uncomment proto module:

```rust
// Before:
// pub mod proto;

// After:
pub mod proto;
```

**Verification**:
```bash
cargo build --package nautilus-dydx --lib
```

### Task 1.3: Verify Proto Access

**Test file**: Create `crates/adapters/dydx/tests/proto_access.rs`

```rust
#[cfg(test)]
mod tests {
    use nautilus_dydx::proto::{
        cosmos_sdk_proto::cosmos::base::v1beta1::Coin,
        dydxprotocol::clob::{Order, OrderId},
    };

    #[test]
    fn test_proto_types_accessible() {
        // Verify we can construct proto types
        let _coin = Coin {
            denom: "adydx".to_string(),
            amount: 1000u128.to_string(),
        };

        let _order_id = OrderId {
            subaccount_id: None,
            client_id: 1,
            order_flags: 0,
            clob_pair_id: 0,
        };
    }
}
```

**Verification**:
```bash
cargo test --package nautilus-dydx proto_access
```

## Phase 2: Enable gRPC Module (1 hour)

### Task 2.1: Remove Stub Code

**File**: `crates/adapters/dydx/src/grpc/mod.rs`

**Action**: Lines 39-53 - Uncomment module declarations:

```rust
// Before:
// TODO: Enable when proto is generated
// pub mod builder;
// pub mod client;
// pub mod order;

// After:
pub mod builder;
pub mod client;
pub mod order;
```

**Action**: Lines 47-53 - Uncomment re-exports:

```rust
// Before:
// TODO: Enable when proto is generated
// pub use builder::TxBuilder;
// pub use client::{DydxGrpcClient, Height, TxHash};
// pub use order::{...};

// After:
pub use builder::TxBuilder;
pub use client::{DydxGrpcClient, Height, TxHash};
pub use order::{
    DEFAULT_RUST_CLIENT_METADATA, OrderBuilder, OrderFlags, OrderGoodUntil, OrderMarketParams,
    SHORT_TERM_ORDER_MAXIMUM_LIFETIME,
};
```

**Action**: Lines 57-133 - Delete entire stub implementation block:

```rust
// DELETE:
// Temporary stubs until proto is generated
#[derive(Debug, Clone)]
pub struct DydxGrpcClient;
// ... (all stub code through line 133)
```

### Task 2.2: Fix Imports in Existing Modules

**File**: `crates/adapters/dydx/src/grpc/client.rs`

**Action**: Verify line 25 has correct import (should already be correct):

```rust
use crate::proto::{
    cosmos_sdk_proto::cosmos::{
        auth::v1beta1::{
            BaseAccount, QueryAccountRequest, query_client::QueryClient as AuthClient,
        },
        // ... rest of imports
    },
};
```

**File**: `crates/adapters/dydx/src/grpc/order.rs`

**Action**: Verify line 30 has correct import (should already be correct):

```rust
use crate::proto::dydxprotocol::{
    clob::{
        Order, OrderId,
        order::{ConditionType, GoodTilOneof, Side as OrderSide, TimeInForce as OrderTimeInForce},
    },
    subaccounts::SubaccountId,
};
```

**Verification**:
```bash
cargo build --package nautilus-dydx
```

## Phase 3: Implement Order Submission (4-6 hours)

### Task 3.1: Remove Placeholder Types

**File**: `crates/adapters/dydx/src/execution/submitter.rs`

**Action**: Lines 31-43 - Delete placeholder enums:

```rust
// DELETE:
// Temporary placeholder types until proto is generated
#[derive(Debug, Clone, Copy)]
pub enum ProtoOrderSide {
    Buy,
    Sell,
}

#[derive(Debug, Clone, Copy)]
pub enum ProtoTimeInForce {
    Unspecified,
    Ioc,
    FillOrKill,
}
```

**Action**: Lines 22-28 - Uncomment and fix imports:

```rust
// Before:
// TODO: Enable when proto is generated
// proto::dydxprotocol::clob::order::{
//     Side as ProtoOrderSide, TimeInForce as ProtoTimeInForce,
// },

// After:
use crate::proto::{
    dydxprotocol::clob::{
        MsgPlaceOrder, MsgCancelOrder, Order,
        order::{Side as ProtoOrderSide, TimeInForce as ProtoTimeInForce},
    },
};
```

### Task 3.2: Add Helper Functions

**File**: `crates/adapters/dydx/src/execution/submitter.rs`

**Action**: Add after struct definition:

```rust
impl OrderSubmitter {
    // ... existing new() method ...

    /// Convert Nautilus OrderSide to proto OrderSide
    fn convert_side(side: OrderSide) -> ProtoOrderSide {
        match side {
            OrderSide::Buy => ProtoOrderSide::Buy,
            OrderSide::Sell => ProtoOrderSide::Sell,
            _ => panic!("Invalid order side for dYdX"),
        }
    }

    /// Convert Nautilus TimeInForce to proto TimeInForce
    fn convert_time_in_force(tif: TimeInForce) -> ProtoTimeInForce {
        match tif {
            TimeInForce::Ioc => ProtoTimeInForce::Ioc,
            TimeInForce::Fok => ProtoTimeInForce::FillOrKill,
            TimeInForce::Gtc => ProtoTimeInForce::Unspecified,
            TimeInForce::Gtt => ProtoTimeInForce::Unspecified,
            TimeInForce::Gtd => ProtoTimeInForce::Unspecified,
            _ => ProtoTimeInForce::Unspecified,
        }
    }
}
```

### Task 3.3: Implement submit_market_order

**File**: `crates/adapters/dydx/src/execution/submitter.rs`

**Action**: Replace stub implementation (lines 83-90) with:

```rust
pub async fn submit_market_order(
    &mut self,
    wallet: &Wallet,
    client_order_id: u32,
    side: OrderSide,
    quantity: Quantity,
    block_height: u32,
) -> Result<(), DydxError> {
    // Build the order using OrderBuilder
    let order = OrderBuilder::new(
        self.get_market_params()?,  // TODO: Pass actual market params
        wallet.address().to_string(),
        self.subaccount_number,
        client_order_id,
    )
    .market(Self::convert_side(side), quantity.as_decimal())
    .until(OrderGoodUntil::Block(block_height))
    .build()
    .map_err(|e| DydxError::Order(format!("Failed to build market order: {e}")))?;

    // Wrap in MsgPlaceOrder
    let msg = MsgPlaceOrder {
        order: Some(order),
    };

    // Convert to Any for transaction
    use crate::proto::ToAny;
    let any_msg = msg.to_any();

    // Build transaction
    let account = wallet.account();
    let tx_builder = TxBuilder::new(
        self.chain_id.clone(),
        "adydx".to_string(),
    )?;
    let tx_bytes = tx_builder.build_for_simulation(&account, vec![any_msg.clone()])?;

    // Simulate to get gas estimate
    let gas_used = self.grpc_client.simulate_tx(tx_bytes).await?;

    // Build final transaction with fee
    let fee = tx_builder.calculate_fee(Some(gas_used))?;
    let tx_raw = tx_builder.build_transaction(&account, vec![any_msg], Some(fee))?;
    let tx_bytes = tx_raw.to_bytes()
        .map_err(|e| DydxError::Encoding(format!("Failed to encode tx: {e}").into()))?;

    // Broadcast
    let tx_hash = self.grpc_client.broadcast_tx(tx_bytes).await?;

    tracing::info!(
        "Market order submitted: client_id={}, tx_hash={}",
        client_order_id,
        tx_hash
    );

    Ok(())
}
```

### Task 3.4: Implement submit_limit_order

**File**: `crates/adapters/dydx/src/execution/submitter.rs`

**Action**: Replace stub implementation (lines 114-126) with similar logic:

```rust
pub async fn submit_limit_order(
    &mut self,
    wallet: &Wallet,
    client_order_id: u32,
    side: OrderSide,
    price: Price,
    quantity: Quantity,
    time_in_force: TimeInForce,
    post_only: bool,
    reduce_only: bool,
    block_height: u32,
    expire_time: Option<i64>,
) -> Result<(), DydxError> {
    let good_until = if let Some(timestamp) = expire_time {
        use chrono::{DateTime, Utc};
        let dt = DateTime::from_timestamp(timestamp, 0)
            .ok_or_else(|| DydxError::InvalidData("Invalid expire timestamp".into()))?;
        OrderGoodUntil::Time(dt)
    } else {
        OrderGoodUntil::Block(block_height)
    };

    let order = OrderBuilder::new(
        self.get_market_params()?,
        wallet.address().to_string(),
        self.subaccount_number,
        client_order_id,
    )
    .limit(
        Self::convert_side(side),
        price.as_decimal(),
        quantity.as_decimal(),
    )
    .time_in_force(Self::convert_time_in_force(time_in_force))
    .reduce_only(reduce_only)
    .until(good_until)
    .build()
    .map_err(|e| DydxError::Order(format!("Failed to build limit order: {e}")))?;

    // Same transaction building and broadcasting logic as market order
    // ... (duplicate code from submit_market_order or extract to helper)

    Ok(())
}
```

### Task 3.5: Implement cancel_order

**File**: `crates/adapters/dydx/src/execution/submitter.rs`

**Action**: Replace stub implementation (lines 139-141) with:

```rust
pub async fn cancel_order(
    &mut self,
    wallet: &Wallet,
    client_order_id: u32,
    block_height: u32,
) -> Result<(), DydxError> {
    use crate::proto::dydxprotocol::clob::{MsgCancelOrder, OrderId, order::GoodTilOneof};
    use crate::proto::dydxprotocol::subaccounts::SubaccountId;

    let order_id = OrderId {
        subaccount_id: Some(SubaccountId {
            owner: wallet.address().to_string(),
            number: self.subaccount_number,
        }),
        client_id: client_order_id,
        order_flags: 0,  // Short-term order
        clob_pair_id: 0,  // TODO: Get from market params
    };

    let msg = MsgCancelOrder {
        order_id: Some(order_id),
        good_til_oneof: Some(GoodTilOneof::GoodTilBlock(block_height)),
    };

    use crate::proto::ToAny;
    let any_msg = msg.to_any();

    // Build and broadcast transaction (same pattern as order submission)
    // ...

    tracing::info!("Order cancelled: client_id={}", client_order_id);
    Ok(())
}
```

### Task 3.6: Implement Conditional Orders

**Files**: `crates/adapters/dydx/src/execution/submitter.rs`

**Action**: Implement stop_market, stop_limit, take_profit_market, take_profit_limit using OrderBuilder:

```rust
pub async fn submit_stop_market_order(
    &mut self,
    wallet: &Wallet,
    client_order_id: u32,
    side: OrderSide,
    trigger_price: Price,
    quantity: Quantity,
    reduce_only: bool,
    block_height: u32,
    expire_time: Option<i64>,
) -> Result<(), DydxError> {
    let good_until = expire_time
        .map(|ts| {
            DateTime::from_timestamp(ts, 0)
                .map(OrderGoodUntil::Time)
                .ok_or_else(|| DydxError::InvalidData("Invalid timestamp".into()))
        })
        .transpose()?
        .unwrap_or(OrderGoodUntil::Block(block_height));

    let order = OrderBuilder::new(
        self.get_market_params()?,
        wallet.address().to_string(),
        self.subaccount_number,
        client_order_id,
    )
    .stop_market(
        Self::convert_side(side),
        trigger_price.as_decimal(),
        quantity.as_decimal(),
    )
    .reduce_only(reduce_only)
    .until(good_until)
    .build()
    .map_err(|e| DydxError::Order(format!("Failed to build stop market order: {e}")))?;

    // Transaction building and broadcasting
    // ...

    Ok(())
}
```

Similar implementations for:
- `submit_stop_limit_order`
- `submit_take_profit_market_order`
- `submit_take_profit_limit_order`

### Task 3.7: Extract Common Transaction Logic

**File**: `crates/adapters/dydx/src/execution/submitter.rs`

**Action**: Add helper method to avoid code duplication:

```rust
async fn broadcast_order_message(
    &mut self,
    wallet: &Wallet,
    msg: impl crate::proto::ToAny,
) -> Result<String, DydxError> {
    use crate::proto::ToAny;

    let any_msg = msg.to_any();
    let account = wallet.account();

    let tx_builder = TxBuilder::new(
        self.chain_id.clone(),
        "adydx".to_string(),
    )?;

    // Simulate for gas
    let tx_bytes = tx_builder.build_for_simulation(&account, vec![any_msg.clone()])?;
    let gas_used = self.grpc_client.simulate_tx(tx_bytes).await?;

    // Build with fee
    let fee = tx_builder.calculate_fee(Some(gas_used))?;
    let tx_raw = tx_builder.build_transaction(&account, vec![any_msg], Some(fee))?;
    let tx_bytes = tx_raw.to_bytes()
        .map_err(|e| DydxError::Encoding(format!("Failed to encode: {e}").into()))?;

    // Broadcast
    let tx_hash = self.grpc_client.broadcast_tx(tx_bytes).await?;
    Ok(tx_hash)
}
```

**Verification**:
```bash
cargo build --package nautilus-dydx
cargo clippy --package nautilus-dydx
```

## Phase 4: Testing (2-3 hours)

### Task 4.1: Unit Tests for Order Building

**File**: `crates/adapters/dydx/tests/order_building.rs`

```rust
use nautilus_dydx::grpc::{OrderBuilder, OrderMarketParams, OrderGoodUntil};
use nautilus_dydx::proto::dydxprotocol::clob::order::Side as ProtoOrderSide;
use rust_decimal_macros::dec;

#[test]
fn test_market_order_proto_creation() {
    let market_params = OrderMarketParams {
        atomic_resolution: -10,
        clob_pair_id: 0,
        oracle_price: Some(dec!(50000)),
        quantum_conversion_exponent: -9,
        step_base_quantums: 1_000_000,
        subticks_per_tick: 100_000,
    };

    let order = OrderBuilder::new(
        market_params,
        "dydx1test".to_string(),
        0,
        1,
    )
    .market(ProtoOrderSide::Buy, dec!(0.01))
    .until(OrderGoodUntil::Block(100))
    .build()
    .unwrap();

    assert_eq!(order.side, ProtoOrderSide::Buy as i32);
    assert_eq!(order.quantums, 100_000_000);
    assert!(!order.reduce_only);
    assert!(order.order_id.is_some());
}

#[test]
fn test_limit_order_proto_creation() {
    let market_params = OrderMarketParams {
        atomic_resolution: -10,
        clob_pair_id: 0,
        oracle_price: Some(dec!(50000)),
        quantum_conversion_exponent: -9,
        step_base_quantums: 1_000_000,
        subticks_per_tick: 100_000,
    };

    let order = OrderBuilder::new(
        market_params,
        "dydx1test".to_string(),
        0,
        2,
    )
    .limit(ProtoOrderSide::Buy, dec!(49000), dec!(0.01))
    .until(OrderGoodUntil::Block(100))
    .build()
    .unwrap();

    assert_eq!(order.side, ProtoOrderSide::Buy as i32);
    assert_eq!(order.subticks, 4_900_000_000);
}
```

### Task 4.2: Integration Test with Testnet

**File**: `crates/adapters/dydx/tests/grpc_integration.rs`

```rust
#[tokio::test]
#[ignore] // Only run with --ignored flag
async fn test_connect_to_testnet() {
    use nautilus_dydx::grpc::DydxGrpcClient;

    let testnet_grpc = vec![
        "https://dydx-testnet-grpc.polkachu.com:23890",
        "https://test-dydx-grpc.kingnodes.com:443",
    ];

    let client = DydxGrpcClient::new_with_fallback(&testnet_grpc)
        .await
        .expect("Failed to connect to testnet");

    let info = client.get_node_info().await
        .expect("Failed to get node info");

    assert!(info.default_node_info.is_some());
}
```

### Task 4.3: Proto Serialization Tests

**File**: `crates/adapters/dydx/tests/proto_serialization.rs`

```rust
use nautilus_dydx::proto::{
    dydxprotocol::clob::{MsgPlaceOrder, Order},
    ToAny,
};
use prost::Message;

#[test]
fn test_order_protobuf_encoding() {
    let order = Order {
        order_id: None,
        side: 1,
        quantums: 100_000_000,
        subticks: 5_000_000_000,
        good_til_oneof: None,
        time_in_force: 0,
        reduce_only: false,
        client_metadata: 4,
        condition_type: 0,
        conditional_order_trigger_subticks: 0,
        twap_parameters: None,
        builder_code_parameters: None,
        order_router_address: String::new(),
    };

    let msg = MsgPlaceOrder { order: Some(order) };

    // Test proto encoding
    let bytes = msg.encode_to_vec();
    assert!(!bytes.is_empty());

    // Test decoding
    let decoded = MsgPlaceOrder::decode(&bytes[..])
        .expect("Failed to decode");
    assert!(decoded.order.is_some());
}

#[test]
fn test_any_conversion() {
    use nautilus_dydx::proto::ToAny;

    let msg = MsgPlaceOrder { order: None };
    let any = msg.to_any();

    assert_eq!(any.type_url, "/dydxprotocol.clob.MsgPlaceOrder");
    assert!(!any.value.is_empty());
}
```

**Verification**:
```bash
# Run all tests
cargo test --package nautilus-dydx

# Run only unit tests
cargo test --package nautilus-dydx --lib

# Run integration tests (including testnet)
cargo test --package nautilus-dydx --test '*' -- --ignored
```

## Phase 5: Documentation (1 hour)

### Task 5.1: Update Module Documentation

**File**: `crates/adapters/dydx/src/execution/mod.rs`

Add module-level documentation:

```rust
//! Execution engine for dYdX v4 protocol.
//!
//! This module provides order submission, modification, and cancellation
//! functionality via gRPC transactions to the dYdX v4 blockchain.
//!
//! # Architecture
//!
//! Orders are submitted using the Cosmos SDK transaction format:
//! 1. Build `Order` proto message using `OrderBuilder`
//! 2. Wrap in `MsgPlaceOrder` transaction message
//! 3. Sign with wallet private key
//! 4. Broadcast to validator via gRPC
//!
//! # Example
//!
//! ```rust,no_run
//! use nautilus_dydx::execution::OrderSubmitter;
//! use nautilus_dydx::grpc::{DydxGrpcClient, Wallet};
//! use nautilus_model::enums::OrderSide;
//! use nautilus_model::types::{Price, Quantity};
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let grpc_client = DydxGrpcClient::new("https://grpc.example.com".into()).await?;
//! let wallet = Wallet::from_mnemonic("your mnemonic...", 0)?;
//! let submitter = OrderSubmitter::new(grpc_client, wallet.address(), 0);
//!
//! submitter.submit_market_order(
//!     &wallet,
//!     1,  // client_order_id
//!     OrderSide::Buy,
//!     Quantity::from("0.01"),
//!     1000,  // block_height
//! ).await?;
//! # Ok(())
//! # }
//! ```
```

### Task 5.2: Add Inline Examples

Add doc comments with examples for each public function in `OrderSubmitter`.

### Task 5.3: Update README (if exists)

Document the proto integration completion in the adapter's README.

## Verification Checklist

After completing all tasks:

- [ ] `cargo build --package nautilus-dydx` succeeds
- [ ] `cargo clippy --package nautilus-dydx` has no warnings
- [ ] `cargo test --package nautilus-dydx` all tests pass
- [ ] Proto types are accessible: `use nautilus_dydx::proto::dydxprotocol::clob::Order;`
- [ ] gRPC client can connect to testnet
- [ ] Order proto messages can be serialized/deserialized
- [ ] `OrderBuilder` produces valid proto messages
- [ ] Transaction signing works with test wallet
- [ ] No remaining `TODO: Enable when proto is generated` comments
- [ ] No stub implementations remain
- [ ] Documentation is complete

## Estimated Timeline

| Phase | Duration | Dependencies |
|-------|----------|--------------|
| Phase 1: Enable Proto | 30 min | None |
| Phase 2: Enable gRPC | 1 hour | Phase 1 |
| Phase 3: Order Submission | 4-6 hours | Phase 2 |
| Phase 4: Testing | 2-3 hours | Phase 3 |
| Phase 5: Documentation | 1 hour | Phase 4 |
| **Total** | **8-10 hours** | Sequential |

## Risk Mitigation

1. **Compilation Errors**: If proto import errors occur, verify `dydx-proto` version matches workspace
2. **Runtime Errors**: Use testnet for initial testing, never mainnet
3. **Transaction Failures**: Implement proper error handling and logging
4. **Gas Estimation**: Use simulation before broadcasting to avoid rejections
5. **Network Issues**: Implement retry logic with exponential backoff

## Success Criteria

The remediation is complete when:

1. All stub code is removed
2. Proto module is enabled and accessible
3. Orders can be built, signed, and broadcast to testnet
4. All tests pass
5. Documentation is complete
6. No compiler warnings or clippy issues
7. Integration tests successfully connect to dYdX testnet

## Next Steps After Completion

1. Implement execution client that uses `OrderSubmitter`
2. Add order state management and tracking
3. Implement WebSocket order updates subscription
4. Add position management
5. Implement risk checks before order submission
6. Add comprehensive error handling and retry logic
7. Performance testing and optimization
