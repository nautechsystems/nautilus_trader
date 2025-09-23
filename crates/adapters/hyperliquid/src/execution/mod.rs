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

//! Hyperliquid execution adapter for Rust Trading Toolkit.
//!
//! This module contains the implementation of an execution adapter that provides
//! connectivity to the Hyperliquid exchange for order management and trade execution.
//!
//! ## Overview
//!
//! The Hyperliquid execution adapter integrates with NautilusTrader's execution
//! framework to provide:
//!
//! - Real-time order placement, cancellation, and modification
//! - Order status updates and trade reporting
//! - Position and account status monitoring
//! - Risk management and order validation
//!
//! ## Components
//!
//! ### HyperliquidExecutionClient
//!
//! The main execution client handles all order management operations:
//!
//! - Connects to Hyperliquid's WebSocket and REST APIs
//! - Translates between NautilusTrader and Hyperliquid order formats
//! - Manages order state and provides real-time updates
//! - Handles authentication and signature generation
//!
//! TODO: Implement HyperliquidExecutionClient
//! TODO: Add order management operations
//! TODO: Add WebSocket event handling
//! TODO: Add authentication and signature handling

// Re-export execution models from the http module
pub use crate::http::models::{
    AssetId, Cloid, DecimalStr, HyperliquidExecAction, HyperliquidExecBuilderFee,
    HyperliquidExecCancelByCloidRequest, HyperliquidExecCancelOrderRequest,
    HyperliquidExecCancelResponseData, HyperliquidExecCancelStatus, HyperliquidExecFilledInfo,
    HyperliquidExecGrouping, HyperliquidExecLimitParams, HyperliquidExecModifyOrderRequest,
    HyperliquidExecModifyResponseData, HyperliquidExecModifyStatus, HyperliquidExecOrderKind,
    HyperliquidExecOrderResponseData, HyperliquidExecOrderStatus, HyperliquidExecPlaceOrderRequest,
    HyperliquidExecRequest, HyperliquidExecResponse, HyperliquidExecResponseData,
    HyperliquidExecRestingInfo, HyperliquidExecTif, HyperliquidExecTpSl,
    HyperliquidExecTriggerParams, HyperliquidExecTwapRequest, OrderId,
};
