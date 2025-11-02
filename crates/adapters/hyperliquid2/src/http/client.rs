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

//! Hyperliquid HTTP client implementation.

use anyhow::Result;
use chrono::Utc;
use reqwest::Client;
use serde_json::Value;

use crate::common::{
    consts::{HYPERLIQUID_HTTP_URL, HYPERLIQUID_INFO_ENDPOINT, HYPERLIQUID_EXCHANGE_ENDPOINT},
    credentials::HyperliquidCredentials,
    models::{
        HyperliquidAllMids, HyperliquidUniverse, HyperliquidOrderRequest, HyperliquidCancelOrderRequest,
        HyperliquidCancelAllOrdersRequest, HyperliquidModifyOrderRequest, HyperliquidUpdateLeverageRequest,
        HyperliquidUpdateIsolatedMarginRequest, HyperliquidUsdcTransferRequest, HyperliquidPortfolioRequest,
        HyperliquidUserFillsRequest, HyperliquidUserFillsByTimeRequest, HyperliquidOpenOrdersRequest,
        HyperliquidHistoricalOrdersRequest, HyperliquidUserStateRequest, HyperliquidPortfolio,
        HyperliquidUserFills, HyperliquidOpenOrders, HyperliquidHistoricalOrders, HyperliquidUserState,
    },
};

/// Hyperliquid HTTP client for API interactions.
#[derive(Debug, Clone)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.adapters")
)]
pub struct HyperliquidHttpClient {
    base_url: String,
    client: Client,
    credentials: Option<HyperliquidCredentials>,
}

impl HyperliquidHttpClient {
    /// Create a new Hyperliquid HTTP client.
    pub fn new(
        base_url: Option<String>,
        credentials: Option<HyperliquidCredentials>,
    ) -> Result<Self> {
        let base_url = base_url.unwrap_or_else(|| HYPERLIQUID_HTTP_URL.to_string());
        let client = Client::new();
        
        Ok(Self {
            base_url,
            client,
            credentials,
        })
    }

    /// Get the credentials associated with this client.
    pub fn credentials(&self) -> Option<&HyperliquidCredentials> {
        self.credentials.as_ref()
    }

    /// Get market universe (list of available assets).
    pub async fn get_universe(&self) -> Result<HyperliquidUniverse> {
        let url = format!("{}{}", self.base_url, HYPERLIQUID_INFO_ENDPOINT);
        let payload = serde_json::json!({
            "type": "meta"
        });

        let response = self.client
            .post(&url)
            .json(&payload)
            .send()
            .await?;

        let universe: HyperliquidUniverse = response.json().await?;
        Ok(universe)
    }

    /// Get all market mid prices.
    pub async fn get_all_mids(&self) -> Result<HyperliquidAllMids> {
        let url = format!("{}{}", self.base_url, HYPERLIQUID_INFO_ENDPOINT);
        let payload = serde_json::json!({
            "type": "allMids"
        });

        let response = self.client
            .post(&url)
            .json(&payload)
            .send()
            .await?;

        let all_mids: HyperliquidAllMids = response.json().await?;
        Ok(all_mids)
    }

    /// Get L2 order book for a specific asset.
    pub async fn get_l2_book(&self, coin: &str) -> Result<Value> {
        let url = format!("{}{}", self.base_url, HYPERLIQUID_INFO_ENDPOINT);
        let payload = serde_json::json!({
            "type": "l2Book",
            "coin": coin
        });

        let response = self.client
            .post(&url)
            .json(&payload)
            .send()
            .await?;

        let l2_book: Value = response.json().await?;
        Ok(l2_book)
    }

    /// Get recent trades for a specific asset.
    pub async fn get_recent_trades(&self, coin: &str) -> Result<Value> {
        let url = format!("{}{}", self.base_url, HYPERLIQUID_INFO_ENDPOINT);
        let payload = serde_json::json!({
            "type": "recentTrades",
            "coin": coin
        });

        let response = self.client
            .post(&url)
            .json(&payload)
            .send()
            .await?;

        let trades: Value = response.json().await?;
        Ok(trades)
    }

    // -- AUTHENTICATED ENDPOINTS --

    /// Place a new order.
    pub async fn place_order(&self, order_request: &HyperliquidOrderRequest) -> Result<Value> {
        let url = format!("{}{}", self.base_url, HYPERLIQUID_EXCHANGE_ENDPOINT);
        let action = serde_json::json!({
            "type": "order",
            "orders": [order_request]
        });

        self.send_authenticated_request(&url, &action).await
    }

    /// Cancel an order.
    pub async fn cancel_order(&self, cancel_request: &HyperliquidCancelOrderRequest) -> Result<Value> {
        let url = format!("{}{}", self.base_url, HYPERLIQUID_EXCHANGE_ENDPOINT);
        let action = serde_json::json!({
            "type": "cancel",
            "cancels": [cancel_request]
        });

        self.send_authenticated_request(&url, &action).await
    }

    /// Cancel all orders for an asset.
    pub async fn cancel_all_orders(&self, cancel_request: &HyperliquidCancelAllOrdersRequest) -> Result<Value> {
        let url = format!("{}{}", self.base_url, HYPERLIQUID_EXCHANGE_ENDPOINT);
        let action = serde_json::json!({
            "type": "cancelByCloid",
            "cancels": [cancel_request]
        });

        self.send_authenticated_request(&url, &action).await
    }

    /// Modify an order.
    pub async fn modify_order(&self, modify_request: &HyperliquidModifyOrderRequest) -> Result<Value> {
        let url = format!("{}{}", self.base_url, HYPERLIQUID_EXCHANGE_ENDPOINT);
        let action = serde_json::json!({
            "type": "modify",
            "oid": modify_request.oid,
            "order": modify_request.order
        });

        self.send_authenticated_request(&url, &action).await
    }

    /// Update leverage for a position.
    pub async fn update_leverage(&self, leverage_request: &HyperliquidUpdateLeverageRequest) -> Result<Value> {
        let url = format!("{}{}", self.base_url, HYPERLIQUID_EXCHANGE_ENDPOINT);
        let action = serde_json::json!({
            "type": "updateLeverage",
            "asset": leverage_request.asset,
            "isCross": leverage_request.is_cross,
            "leverage": leverage_request.leverage
        });

        self.send_authenticated_request(&url, &action).await
    }

    /// Update isolated margin.
    pub async fn update_isolated_margin(&self, margin_request: &HyperliquidUpdateIsolatedMarginRequest) -> Result<Value> {
        let url = format!("{}{}", self.base_url, HYPERLIQUID_EXCHANGE_ENDPOINT);
        let action = serde_json::json!({
            "type": "updateIsolatedMargin",
            "asset": margin_request.asset,
            "isCross": margin_request.is_cross,
            "ntli": margin_request.ntli
        });

        self.send_authenticated_request(&url, &action).await
    }

    /// Transfer USDC to another address.
    pub async fn transfer_usdc(&self, transfer_request: &HyperliquidUsdcTransferRequest) -> Result<Value> {
        let url = format!("{}{}", self.base_url, HYPERLIQUID_EXCHANGE_ENDPOINT);
        let action = serde_json::json!({
            "type": "usdSend",
            "hyperliquidChain": "Mainnet",
            "signatureChainId": "0xa4b1",
            "destination": transfer_request.destination,
            "amount": transfer_request.amount,
            "time": transfer_request.time
        });

        self.send_authenticated_request(&url, &action).await
    }

    // -- USER DATA ENDPOINTS --

    /// Get user's clearinghouse state (positions, margin, etc).
    pub async fn get_user_state(&self, user_address: &str) -> Result<HyperliquidUserState> {
        let url = format!("{}{}", self.base_url, HYPERLIQUID_INFO_ENDPOINT);
        let payload = HyperliquidUserStateRequest {
            type_: "clearinghouseState".to_string(),
            user: user_address.to_string(),
        };

        let response = self.client
            .post(&url)
            .json(&payload)
            .send()
            .await?;

        let user_state: HyperliquidUserState = response.json().await?;
        Ok(user_state)
    }

    /// Get user's portfolio information.
    pub async fn get_portfolio(&self, user_address: &str) -> Result<HyperliquidPortfolio> {
        let url = format!("{}{}", self.base_url, HYPERLIQUID_INFO_ENDPOINT);
        let payload = HyperliquidPortfolioRequest {
            type_: "portfolio".to_string(),
            user: user_address.to_string(),
        };

        let response = self.client
            .post(&url)
            .json(&payload)
            .send()
            .await?;

        let portfolio: HyperliquidPortfolio = response.json().await?;
        Ok(portfolio)
    }

    /// Get user's recent fills.
    pub async fn get_user_fills(&self, user_address: &str) -> Result<HyperliquidUserFills> {
        let url = format!("{}{}", self.base_url, HYPERLIQUID_INFO_ENDPOINT);
        let payload = HyperliquidUserFillsRequest {
            type_: "userFills".to_string(),
            user: user_address.to_string(),
        };

        let response = self.client
            .post(&url)
            .json(&payload)
            .send()
            .await?;

        let fills: HyperliquidUserFills = response.json().await?;
        Ok(fills)
    }

    /// Get user's fills by time range.
    pub async fn get_user_fills_by_time(&self, user_address: &str, start_time: u64, end_time: u64) -> Result<HyperliquidUserFills> {
        let url = format!("{}{}", self.base_url, HYPERLIQUID_INFO_ENDPOINT);
        let payload = HyperliquidUserFillsByTimeRequest {
            type_: "userFillsByTime".to_string(),
            user: user_address.to_string(),
            start_time,
            end_time,
        };

        let response = self.client
            .post(&url)
            .json(&payload)
            .send()
            .await?;

        let fills: HyperliquidUserFills = response.json().await?;
        Ok(fills)
    }

    /// Get user's open orders.
    pub async fn get_open_orders(&self, user_address: &str) -> Result<HyperliquidOpenOrders> {
        let url = format!("{}{}", self.base_url, HYPERLIQUID_INFO_ENDPOINT);
        let payload = HyperliquidOpenOrdersRequest {
            type_: "openOrders".to_string(),
            user: user_address.to_string(),
        };

        let response = self.client
            .post(&url)
            .json(&payload)
            .send()
            .await?;

        let orders: HyperliquidOpenOrders = response.json().await?;
        Ok(orders)
    }

    /// Get user's historical orders.
    pub async fn get_historical_orders(&self, user_address: &str) -> Result<HyperliquidHistoricalOrders> {
        let url = format!("{}{}", self.base_url, HYPERLIQUID_INFO_ENDPOINT);
        let payload = HyperliquidHistoricalOrdersRequest {
            type_: "historicalOrders".to_string(),
            user: user_address.to_string(),
        };

        let response = self.client
            .post(&url)
            .json(&payload)
            .send()
            .await?;

        let orders: HyperliquidHistoricalOrders = response.json().await?;
        Ok(orders)
    }

    // -- PRIVATE METHODS --

    /// Send an authenticated request to the exchange endpoint.
    async fn send_authenticated_request(&self, url: &str, action: &Value) -> Result<Value> {
        if let Some(credentials) = &self.credentials {
            // Create timestamp
            let timestamp = Utc::now().timestamp_millis() as u64;
            
            // Sign the action
            let action_json = serde_json::to_string(action)?;
            let message = format!("{}{}", action_json, timestamp);
            let signature = credentials.sign_message(&message)?;
            
            // Create signed request
            let signed_request = serde_json::json!({
                "action": action,
                "nonce": timestamp,
                "signature": signature,
                "vaultAddress": null
            });

            let response = self.client
                .post(url)
                .json(&signed_request)
                .send()
                .await?;

            let result: Value = response.json().await?;
            Ok(result)
        } else {
            anyhow::bail!("Authentication required for this endpoint")
        }
    }
}
