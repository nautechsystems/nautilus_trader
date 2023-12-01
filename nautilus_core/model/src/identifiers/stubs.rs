// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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

use nautilus_core::uuid::UUID4;
use rstest::fixture;

use crate::identifiers::{
    account_id::AccountId, client_id::ClientId, client_order_id::ClientOrderId,
    component_id::ComponentId, exec_algorithm_id::ExecAlgorithmId, instrument_id::InstrumentId,
    order_list_id::OrderListId, position_id::PositionId, strategy_id::StrategyId, symbol::Symbol,
    trade_id::TradeId, trader_id::TraderId, venue::Venue, venue_order_id::VenueOrderId,
};

// ---- AccountId ----

#[fixture]
pub fn account_id() -> AccountId {
    AccountId::from("SIM-001")
}

#[fixture]
pub fn account_ib() -> AccountId {
    AccountId::from("IB-1234567890")
}

// ---- ClientId ----

#[fixture]
pub fn client_id_binance() -> ClientId {
    ClientId::from("BINANCE")
}

#[fixture]
pub fn client_id_dydx() -> ClientId {
    ClientId::from("COINBASE")
}

// ---- ClientOrderId ----

#[fixture]
pub fn client_order_id() -> ClientOrderId {
    ClientOrderId::from("O-20200814-102234-001-001-1")
}

// ---- ComponentId ----

#[fixture]
pub fn component_risk_engine() -> ComponentId {
    ComponentId::from("RiskEngine")
}

// ---- ExecAlgorithmId ----

#[fixture]
pub fn exec_algorithm_id() -> ExecAlgorithmId {
    ExecAlgorithmId::from("001")
}

// ---- InstrumentId ----

#[fixture]
pub fn instrument_id_eth_usdt_binance() -> InstrumentId {
    InstrumentId::from("ETHUSDT.BINANCE")
}

#[fixture]
pub fn instrument_id_btc_usdt() -> InstrumentId {
    InstrumentId::from("BTCUSDT.COINBASE")
}

// ---- OrderListId ----

#[fixture]
pub fn order_list_id_test() -> OrderListId {
    OrderListId::from("001")
}

// ---- PositionId ----

#[fixture]
pub fn position_id_test() -> PositionId {
    PositionId::from("P-123456789")
}

// ---- StrategyId ----

#[fixture]
pub fn strategy_id_ema_cross() -> StrategyId {
    StrategyId::from("EMACross-001")
}

// ---- Symbol ----

#[fixture]
pub fn symbol_eth_perp() -> Symbol {
    Symbol::from("ETH-PERP")
}

#[fixture]
pub fn symbol_aud_usd() -> Symbol {
    Symbol::from("AUDUSD")
}

// ---- TradeId ----

#[fixture]
pub fn trade_id() -> TradeId {
    TradeId::from("1234567890")
}

// ---- TraderId ----
#[fixture]
pub fn trader_id() -> TraderId {
    TraderId::from("TRADER-001")
}

// ---- Venue ----

#[fixture]
pub fn venue_binance() -> Venue {
    Venue::from("BINANCE")
}

#[fixture]
pub fn venue_sim() -> Venue {
    Venue::from("SIM")
}

// ---- VenueOrderId ----
#[fixture]
pub fn venue_order_id() -> VenueOrderId {
    VenueOrderId::from("001")
}

// ---- UUID ----
#[fixture]
pub fn uuid4() -> UUID4 {
    UUID4::from("16578139-a945-4b65-b46c-bc131a15d8e7")
}
