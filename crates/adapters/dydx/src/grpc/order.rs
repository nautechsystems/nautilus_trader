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

//! Order types and builders for dYdX v4.
//!
//! This module provides order construction utilities for placing orders on dYdX v4.
//! dYdX supports two order lifetime types:
//!
//! - **Short-term orders**: Expire by block height (max 20 blocks).
//! - **Long-term orders**: Expire by timestamp.
//!
//! See [dYdX order types](https://help.dydx.trade/en/articles/166985-short-term-vs-long-term-order-types).

use chrono::{DateTime, Utc};
use dydx_proto::dydxprotocol::clob::{
    Order, OrderId,
    order::{ConditionType, Side as OrderSide, TimeInForce as OrderTimeInForce},
};
use rust_decimal::Decimal;

/// Maximum short-term order lifetime in blocks.
///
/// See also [short-term vs long-term orders](https://help.dydx.trade/en/articles/166985-short-term-vs-long-term-order-types).
pub const SHORT_TERM_ORDER_MAXIMUM_LIFETIME: u32 = 20;

/// Value used to identify the Rust client in order metadata.
pub const DEFAULT_RUST_CLIENT_METADATA: u32 = 4;

/// Order [expiration types](https://docs.dydx.xyz/concepts/trading/orders#comparison).
#[derive(Clone, Debug)]
pub enum OrderGoodUntil {
    /// Block expiration is used for short-term orders.
    /// The order expires after the specified block height.
    Block(u32),
    /// Time expiration is used for long-term orders.
    /// The order expires at the specified timestamp.
    Time(DateTime<Utc>),
}

/// Order type enumeration.
#[derive(Clone, Debug)]
pub enum OrderType {
    /// Limit order.
    Limit,
    /// Market order.
    Market,
    /// Stop limit order.
    StopLimit,
    /// Stop market order.
    StopMarket,
    /// Take profit order.
    TakeProfit,
    /// Take profit market order.
    TakeProfitMarket,
}

/// Order flags indicating order lifetime and execution type.
#[derive(Clone, Debug)]
pub enum OrderFlags {
    /// Short-term order (expires by block height).
    ShortTerm,
    /// Long-term order (expires by timestamp).
    LongTerm,
    /// Conditional order (triggered by trigger price).
    Conditional,
}

/// Market parameters required for price and size quantizations.
///
/// These quantizations are required for `Order` placement.
/// See also [how to interpret block data for trades](https://docs.dydx.exchange/api_integration-guides/how_to_interpret_block_data_for_trades).
#[derive(Clone, Debug)]
pub struct OrderMarketParams {
    /// Atomic resolution.
    pub atomic_resolution: i32,
    /// CLOB pair ID.
    pub clob_pair_id: u32,
    /// Oracle price.
    pub oracle_price: Option<Decimal>,
    /// Quantum conversion exponent.
    pub quantum_conversion_exponent: i32,
    /// Step base quantums.
    pub step_base_quantums: u64,
    /// Subticks per tick.
    pub subticks_per_tick: u32,
}

impl OrderMarketParams {
    /// Convert price into subticks.
    ///
    /// # Errors
    ///
    /// Returns an error if conversion fails.
    pub fn quantize_price(&self, price: Decimal) -> Result<u64, anyhow::Error> {
        const QUOTE_QUANTUMS_ATOMIC_RESOLUTION: i32 = -6;
        let scale = -(self.atomic_resolution
            - self.quantum_conversion_exponent
            - QUOTE_QUANTUMS_ATOMIC_RESOLUTION);

        let factor = Decimal::new(1, scale.unsigned_abs());
        let raw_subticks = price * factor;
        let subticks_per_tick = Decimal::from(self.subticks_per_tick);
        let quantums = Self::quantize(&raw_subticks, &subticks_per_tick);
        let result = quantums.max(subticks_per_tick);

        result
            .to_u64()
            .ok_or_else(|| anyhow::anyhow!("Failed to convert price to u64"))
    }

    /// Convert decimal into quantums.
    ///
    /// # Errors
    ///
    /// Returns an error if conversion fails.
    pub fn quantize_quantity(&self, quantity: Decimal) -> Result<u64, anyhow::Error> {
        let factor = Decimal::new(1, self.atomic_resolution.unsigned_abs());
        let raw_quantums = quantity * factor;
        let step_base_quantums = Decimal::from(self.step_base_quantums);
        let quantums = Self::quantize(&raw_quantums, &step_base_quantums);
        let result = quantums.max(step_base_quantums);

        result
            .to_u64()
            .ok_or_else(|| anyhow::anyhow!("Failed to convert quantity to u64"))
    }

    /// A `round`-like function that quantizes a `value` to the `fraction`.
    fn quantize(value: &Decimal, fraction: &Decimal) -> Decimal {
        (value / fraction).round() * fraction
    }

    /// Get orderbook pair id.
    #[must_use]
    pub fn clob_pair_id(&self) -> u32 {
        self.clob_pair_id
    }
}

/// [`Order`] builder.
///
/// Note that the price input to the `OrderBuilder` is in the "common" units of the perpetual/currency,
/// not the quantized/atomic value.
///
/// Two main classes of orders in dYdX from persistence perspective are
/// [short-term and long-term (stateful) orders](https://docs.dydx.xyz/concepts/trading/orders#short-term-vs-long-term).
///
/// For different types of orders see also [Stop-Limit Versus Stop-Loss](https://dydx.exchange/crypto-learning/stop-limit-versus-stop-loss)
/// and [Perpetual order types on dYdX Chain](https://help.dydx.trade/en/articles/166981-perpetual-order-types-on-dydx-chain).
#[derive(Clone, Debug)]
pub struct OrderBuilder {
    market_params: OrderMarketParams,
    subaccount_owner: String,
    subaccount_number: u32,
    client_id: u32,
    flags: OrderFlags,
    side: Option<OrderSide>,
    order_type: Option<OrderType>,
    size: Option<Decimal>,
    price: Option<Decimal>,
    time_in_force: Option<OrderTimeInForce>,
    reduce_only: Option<bool>,
    until: Option<OrderGoodUntil>,
    trigger_price: Option<Decimal>,
    condition_type: Option<ConditionType>,
}

impl OrderBuilder {
    /// Create a new [`Order`] builder.
    #[must_use]
    pub fn new(
        market_params: OrderMarketParams,
        subaccount_owner: String,
        subaccount_number: u32,
        client_id: u32,
    ) -> Self {
        Self {
            market_params,
            subaccount_owner,
            subaccount_number,
            client_id,
            flags: OrderFlags::ShortTerm,
            side: Some(OrderSide::Buy),
            order_type: Some(OrderType::Market),
            size: None,
            price: None,
            time_in_force: None,
            reduce_only: None,
            until: None,
            trigger_price: None,
            condition_type: None,
        }
    }

    /// Set as Market order.
    ///
    /// An instruction to immediately buy or sell an asset at the best available price when the order is placed.
    pub fn market(mut self, side: OrderSide, size: Decimal) -> Self {
        self.order_type = Some(OrderType::Market);
        self.side = Some(side);
        self.size = Some(size);
        self
    }

    /// Set as Limit order.
    ///
    /// With a limit order, a trader specifies the price at which they're willing to buy or sell an asset.
    /// Unlike market orders, limit orders don't go into effect until the market price hits a trader's "limit price."
    pub fn limit(mut self, side: OrderSide, price: Decimal, size: Decimal) -> Self {
        self.order_type = Some(OrderType::Limit);
        self.price = Some(price);
        self.side = Some(side);
        self.size = Some(size);
        self
    }

    /// Set as Stop Limit order.
    ///
    /// Stop-limit orders use a stop `trigger_price` and a limit `price` to give investors greater control over their trades.
    pub fn stop_limit(
        mut self,
        side: OrderSide,
        price: Decimal,
        trigger_price: Decimal,
        size: Decimal,
    ) -> Self {
        self.order_type = Some(OrderType::StopLimit);
        self.price = Some(price);
        self.trigger_price = Some(trigger_price);
        self.side = Some(side);
        self.size = Some(size);
        self.conditional()
    }

    /// Set as Stop Market order.
    ///
    /// When using a stop order, the trader sets a `trigger_price` to trigger a buy or sell order on their exchange.
    pub fn stop_market(mut self, side: OrderSide, trigger_price: Decimal, size: Decimal) -> Self {
        self.order_type = Some(OrderType::StopMarket);
        self.trigger_price = Some(trigger_price);
        self.side = Some(side);
        self.size = Some(size);
        self.conditional()
    }

    /// Set as Take Profit Limit order.
    ///
    /// The order enters in force if the price reaches `trigger_price` and is executed at `price` after that.
    pub fn take_profit_limit(
        mut self,
        side: OrderSide,
        price: Decimal,
        trigger_price: Decimal,
        size: Decimal,
    ) -> Self {
        self.order_type = Some(OrderType::TakeProfit);
        self.price = Some(price);
        self.trigger_price = Some(trigger_price);
        self.side = Some(side);
        self.size = Some(size);
        self.conditional()
    }

    /// Set as Take Profit Market order.
    ///
    /// The order enters in force if the price reaches `trigger_price` and converts to an ordinary market order.
    pub fn take_profit_market(
        mut self,
        side: OrderSide,
        trigger_price: Decimal,
        size: Decimal,
    ) -> Self {
        self.order_type = Some(OrderType::TakeProfitMarket);
        self.trigger_price = Some(trigger_price);
        self.side = Some(side);
        self.size = Some(size);
        self.conditional()
    }

    /// Set order as a long-term order.
    pub fn long_term(mut self) -> Self {
        self.flags = OrderFlags::LongTerm;
        self
    }

    /// Set order as a short-term order.
    pub fn short_term(mut self) -> Self {
        self.flags = OrderFlags::ShortTerm;
        self
    }

    /// Set order as a conditional order, triggered using `trigger_price`.
    pub fn conditional(mut self) -> Self {
        self.flags = OrderFlags::Conditional;
        self
    }

    /// Set the limit price for Limit orders.
    pub fn price(mut self, price: Decimal) -> Self {
        self.price = Some(price);
        self
    }

    /// Set position size.
    pub fn size(mut self, size: Decimal) -> Self {
        self.size = Some(size);
        self
    }

    /// Set [time execution options](https://docs.dydx.xyz/types/time_in_force#time-in-force).
    pub fn time_in_force(mut self, tif: OrderTimeInForce) -> Self {
        self.time_in_force = Some(tif);
        self
    }

    /// Set an order as [reduce-only](https://docs.dydx.xyz/concepts/trading/orders#types).
    pub fn reduce_only(mut self, reduce: bool) -> Self {
        self.reduce_only = Some(reduce);
        self
    }

    /// Set order's expiration.
    pub fn until(mut self, gtof: OrderGoodUntil) -> Self {
        self.until = Some(gtof);
        self
    }

    /// Build the order.
    ///
    /// # Errors
    ///
    /// Returns an error if the order parameters are invalid.
    pub fn build(self) -> Result<Order, anyhow::Error> {
        let side = self
            .side
            .ok_or_else(|| anyhow::anyhow!("Order side not set"))?;
        let size = self
            .size
            .ok_or_else(|| anyhow::anyhow!("Order size not set"))?;

        // Quantize size
        let quantums = self.market_params.quantize_quantity(size)?;

        // Build order ID
        let order_id = Some(OrderId {
            subaccount_id: Some(dydx_proto::dydxprotocol::subaccounts::SubaccountId {
                owner: self.subaccount_owner.clone(),
                number: self.subaccount_number,
            }),
            client_id: self.client_id,
            order_flags: match self.flags {
                OrderFlags::ShortTerm => 0,
                OrderFlags::LongTerm => 64,
                OrderFlags::Conditional => 32,
            },
            clob_pair_id: self.market_params.clob_pair_id,
        });

        // Set good til oneof
        let good_til_oneof = if let Some(until) = self.until {
            match until {
                OrderGoodUntil::Block(height) => {
                    Some(dydx_proto::dydxprotocol::clob::order::GoodTilOneof::GoodTilBlock(height))
                }
                OrderGoodUntil::Time(time) => Some(
                    dydx_proto::dydxprotocol::clob::order::GoodTilOneof::GoodTilBlockTime(
                        time.timestamp().try_into()?,
                    ),
                ),
            }
        } else {
            None
        };

        // Quantize price if provided
        let subticks = if let Some(price) = self.price {
            self.market_params.quantize_price(price)?
        } else {
            0
        };

        Ok(Order {
            order_id,
            side: side as i32,
            quantums,
            subticks,
            good_til_oneof,
            time_in_force: self.time_in_force.map(|tif| tif as i32).unwrap_or(0),
            reduce_only: self.reduce_only.unwrap_or(false),
            client_metadata: DEFAULT_RUST_CLIENT_METADATA,
            condition_type: self.condition_type.map(|ct| ct as i32).unwrap_or(0),
            conditional_order_trigger_subticks: self
                .trigger_price
                .map(|tp| self.market_params.quantize_price(tp))
                .transpose()?
                .unwrap_or(0),
            twap_parameters: None,
            builder_code_parameters: None,
            order_router_address: String::new(),
        })
    }
}

impl Default for OrderBuilder {
    fn default() -> Self {
        Self {
            market_params: OrderMarketParams {
                atomic_resolution: -10,
                clob_pair_id: 0,
                oracle_price: None,
                quantum_conversion_exponent: -9,
                step_base_quantums: 1_000_000,
                subticks_per_tick: 100_000,
            },
            subaccount_owner: String::new(),
            subaccount_number: 0,
            client_id: 0,
            flags: OrderFlags::ShortTerm,
            side: Some(OrderSide::Buy),
            order_type: Some(OrderType::Market),
            size: None,
            price: None,
            time_in_force: None,
            reduce_only: None,
            until: None,
            trigger_price: None,
            condition_type: None,
        }
    }
}
