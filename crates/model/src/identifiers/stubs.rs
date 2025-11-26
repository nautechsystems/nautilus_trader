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

//! Helper functions for stubbing identifiers in tests.

use nautilus_core::UUID4;
use rstest::fixture;

use super::{
    AccountId, ClientId, ClientOrderId, ComponentId, ExecAlgorithmId, InstrumentId, OrderListId,
    PositionId, StrategyId, Symbol, TradeId, TraderId, Venue, VenueOrderId,
};

/// Returns a stub trader ID.
#[fixture]
pub fn trader_id() -> TraderId {
    TraderId::new("TRADER-001")
}

/// Returns a stub strategy ID for an EMA cross strategy.
#[fixture]
pub fn strategy_id_ema_cross() -> StrategyId {
    StrategyId::new("EMACross-001")
}

/// Returns a stub UUID4.
#[fixture]
pub fn uuid4() -> UUID4 {
    UUID4::from("16578139-a945-4b65-b46c-bc131a15d8e7")
}

/// Returns a stub account ID.
#[fixture]
pub fn account_id() -> AccountId {
    AccountId::new("SIM-001")
}

/// Returns a stub client order ID.
#[fixture]
pub fn client_order_id() -> ClientOrderId {
    ClientOrderId::new("O-19700101-000000-001-001-1")
}

/// Returns a stub venue order ID.
#[fixture]
pub fn venue_order_id() -> VenueOrderId {
    VenueOrderId::new("001")
}

/// Returns a stub position ID.
#[fixture]
pub fn position_id() -> PositionId {
    PositionId::new("P-001")
}

/// Returns a stub instrument ID for BTC/USDT.
#[fixture]
pub fn instrument_id_btc_usdt() -> InstrumentId {
    InstrumentId::new(Symbol::new("BTCUSDT"), Venue::new("COINBASE"))
}

/// Returns a stub instrument ID for AUD/USD.
#[fixture]
pub fn instrument_id_aud_usd() -> InstrumentId {
    InstrumentId::new(Symbol::new("AUD/USD"), Venue::new("SIM"))
}

/// Returns a stub instrument ID for AUD/USD on SIM venue.
#[fixture]
pub fn instrument_id_aud_usd_sim() -> InstrumentId {
    InstrumentId::new(Symbol::new("AUD/USD"), Venue::new("SIM"))
}

/// Returns a stub trade ID.
#[fixture]
pub fn trade_id() -> TradeId {
    TradeId::new("1234567890")
}

/// Returns a stub symbol for ETH-PERP.
#[fixture]
pub fn symbol_eth_perp() -> Symbol {
    Symbol::new("ETH-PERP")
}

/// Returns a stub symbol for AUD/USD.
#[fixture]
pub fn symbol_aud_usd() -> Symbol {
    Symbol::new("AUD/USD")
}

/// Returns a stub venue for BINANCE.
#[fixture]
pub fn venue_binance() -> Venue {
    Venue::new("BINANCE")
}

/// Returns a stub venue for SIM.
#[fixture]
pub fn venue_sim() -> Venue {
    Venue::new("SIM")
}

/// Returns a stub client ID for BINANCE.
#[fixture]
pub fn client_id_binance() -> ClientId {
    ClientId::new("BINANCE")
}

/// Returns a stub client ID for dYdX.
#[fixture]
pub fn client_id_dydx() -> ClientId {
    ClientId::new("DYDX")
}

/// Returns a stub position ID for tests.
#[fixture]
pub fn position_id_test() -> PositionId {
    PositionId::new("P-123456789")
}

/// Returns a stub order list ID for tests.
#[fixture]
pub fn order_list_id_test() -> OrderListId {
    OrderListId::new("001")
}

/// Returns a stub instrument ID for ETH/USDT on BINANCE.
#[fixture]
pub fn instrument_id_eth_usdt_binance() -> InstrumentId {
    InstrumentId::new(Symbol::new("ETHUSDT"), Venue::new("BINANCE"))
}

/// Returns a stub account ID for Interactive Brokers.
#[fixture]
pub fn account_ib() -> AccountId {
    AccountId::new("IB-1234567890")
}

/// Returns a stub component ID for risk engine.
#[fixture]
pub fn component_risk_engine() -> ComponentId {
    ComponentId::new("RiskEngine")
}

/// Returns a stub execution algorithm ID.
#[fixture]
pub fn exec_algorithm_id() -> ExecAlgorithmId {
    ExecAlgorithmId::new("001")
}
