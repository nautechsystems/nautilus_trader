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

//! Cap'n Proto serialization integration tests for identifier types.

#![cfg(feature = "capnp")]

use nautilus_model::identifiers::{
    AccountId, ActorId, ClientId, ClientOrderId, ComponentId, ExecAlgorithmId, InstrumentId,
    OrderListId, PositionId, StrategyId, Symbol, TradeId, TraderId, Venue, VenueOrderId,
};
use nautilus_serialization::capnp::{FromCapnp, ToCapnp, identifiers_capnp};
use rstest::rstest;

#[rstest]
fn test_trader_id_roundtrip() {
    let trader_id = TraderId::from("TRADER-001");

    let mut message = capnp::message::Builder::new_default();
    let builder = message.init_root::<identifiers_capnp::trader_id::Builder>();
    trader_id.to_capnp(builder);

    let mut bytes = Vec::new();
    capnp::serialize::write_message(&mut bytes, &message).unwrap();

    let reader =
        capnp::serialize::read_message(&mut &bytes[..], capnp::message::ReaderOptions::new())
            .unwrap();
    let root = reader
        .get_root::<identifiers_capnp::trader_id::Reader>()
        .unwrap();
    let decoded = TraderId::from_capnp(root).unwrap();

    assert_eq!(trader_id, decoded);
}

#[rstest]
fn test_strategy_id_roundtrip() {
    let strategy_id = StrategyId::from("EMACross-001");

    let mut message = capnp::message::Builder::new_default();
    let builder = message.init_root::<identifiers_capnp::strategy_id::Builder>();
    strategy_id.to_capnp(builder);

    let mut bytes = Vec::new();
    capnp::serialize::write_message(&mut bytes, &message).unwrap();

    let reader =
        capnp::serialize::read_message(&mut &bytes[..], capnp::message::ReaderOptions::new())
            .unwrap();
    let root = reader
        .get_root::<identifiers_capnp::strategy_id::Reader>()
        .unwrap();
    let decoded = StrategyId::from_capnp(root).unwrap();

    assert_eq!(strategy_id, decoded);
}

#[rstest]
fn test_actor_id_roundtrip() {
    let actor_id = ActorId::from("ACTOR-001");

    let mut message = capnp::message::Builder::new_default();
    let builder = message.init_root::<identifiers_capnp::actor_id::Builder>();
    actor_id.to_capnp(builder);

    let mut bytes = Vec::new();
    capnp::serialize::write_message(&mut bytes, &message).unwrap();

    let reader =
        capnp::serialize::read_message(&mut &bytes[..], capnp::message::ReaderOptions::new())
            .unwrap();
    let root = reader
        .get_root::<identifiers_capnp::actor_id::Reader>()
        .unwrap();
    let decoded = ActorId::from_capnp(root).unwrap();

    assert_eq!(actor_id, decoded);
}

#[rstest]
fn test_account_id_roundtrip() {
    let account_id = AccountId::from("ACC-001");

    let mut message = capnp::message::Builder::new_default();
    let builder = message.init_root::<identifiers_capnp::account_id::Builder>();
    account_id.to_capnp(builder);

    let mut bytes = Vec::new();
    capnp::serialize::write_message(&mut bytes, &message).unwrap();

    let reader =
        capnp::serialize::read_message(&mut &bytes[..], capnp::message::ReaderOptions::new())
            .unwrap();
    let root = reader
        .get_root::<identifiers_capnp::account_id::Reader>()
        .unwrap();
    let decoded = AccountId::from_capnp(root).unwrap();

    assert_eq!(account_id, decoded);
}

#[rstest]
fn test_client_id_roundtrip() {
    let client_id = ClientId::from("BINANCE");

    let mut message = capnp::message::Builder::new_default();
    let builder = message.init_root::<identifiers_capnp::client_id::Builder>();
    client_id.to_capnp(builder);

    let mut bytes = Vec::new();
    capnp::serialize::write_message(&mut bytes, &message).unwrap();

    let reader =
        capnp::serialize::read_message(&mut &bytes[..], capnp::message::ReaderOptions::new())
            .unwrap();
    let root = reader
        .get_root::<identifiers_capnp::client_id::Reader>()
        .unwrap();
    let decoded = ClientId::from_capnp(root).unwrap();

    assert_eq!(client_id, decoded);
}

#[rstest]
fn test_client_order_id_roundtrip() {
    let client_order_id = ClientOrderId::from("O-20240101-001");

    let mut message = capnp::message::Builder::new_default();
    let builder = message.init_root::<identifiers_capnp::client_order_id::Builder>();
    client_order_id.to_capnp(builder);

    let mut bytes = Vec::new();
    capnp::serialize::write_message(&mut bytes, &message).unwrap();

    let reader =
        capnp::serialize::read_message(&mut &bytes[..], capnp::message::ReaderOptions::new())
            .unwrap();
    let root = reader
        .get_root::<identifiers_capnp::client_order_id::Reader>()
        .unwrap();
    let decoded = ClientOrderId::from_capnp(root).unwrap();

    assert_eq!(client_order_id, decoded);
}

#[rstest]
fn test_venue_order_id_roundtrip() {
    let venue_order_id = VenueOrderId::from("123456789");

    let mut message = capnp::message::Builder::new_default();
    let builder = message.init_root::<identifiers_capnp::venue_order_id::Builder>();
    venue_order_id.to_capnp(builder);

    let mut bytes = Vec::new();
    capnp::serialize::write_message(&mut bytes, &message).unwrap();

    let reader =
        capnp::serialize::read_message(&mut &bytes[..], capnp::message::ReaderOptions::new())
            .unwrap();
    let root = reader
        .get_root::<identifiers_capnp::venue_order_id::Reader>()
        .unwrap();
    let decoded = VenueOrderId::from_capnp(root).unwrap();

    assert_eq!(venue_order_id, decoded);
}

#[rstest]
fn test_trade_id_roundtrip() {
    let trade_id = TradeId::new("T-20240101-001");

    let mut message = capnp::message::Builder::new_default();
    let builder = message.init_root::<identifiers_capnp::trade_id::Builder>();
    trade_id.to_capnp(builder);

    let mut bytes = Vec::new();
    capnp::serialize::write_message(&mut bytes, &message).unwrap();

    let reader =
        capnp::serialize::read_message(&mut &bytes[..], capnp::message::ReaderOptions::new())
            .unwrap();
    let root = reader
        .get_root::<identifiers_capnp::trade_id::Reader>()
        .unwrap();
    let decoded = TradeId::from_capnp(root).unwrap();

    assert_eq!(trade_id, decoded);
}

#[rstest]
fn test_position_id_roundtrip() {
    let position_id = PositionId::from("P-001");

    let mut message = capnp::message::Builder::new_default();
    let builder = message.init_root::<identifiers_capnp::position_id::Builder>();
    position_id.to_capnp(builder);

    let mut bytes = Vec::new();
    capnp::serialize::write_message(&mut bytes, &message).unwrap();

    let reader =
        capnp::serialize::read_message(&mut &bytes[..], capnp::message::ReaderOptions::new())
            .unwrap();
    let root = reader
        .get_root::<identifiers_capnp::position_id::Reader>()
        .unwrap();
    let decoded = PositionId::from_capnp(root).unwrap();

    assert_eq!(position_id, decoded);
}

#[rstest]
fn test_exec_algorithm_id_roundtrip() {
    let exec_algorithm_id = ExecAlgorithmId::from("TWAP");

    let mut message = capnp::message::Builder::new_default();
    let builder = message.init_root::<identifiers_capnp::exec_algorithm_id::Builder>();
    exec_algorithm_id.to_capnp(builder);

    let mut bytes = Vec::new();
    capnp::serialize::write_message(&mut bytes, &message).unwrap();

    let reader =
        capnp::serialize::read_message(&mut &bytes[..], capnp::message::ReaderOptions::new())
            .unwrap();
    let root = reader
        .get_root::<identifiers_capnp::exec_algorithm_id::Reader>()
        .unwrap();
    let decoded = ExecAlgorithmId::from_capnp(root).unwrap();

    assert_eq!(exec_algorithm_id, decoded);
}

#[rstest]
fn test_component_id_roundtrip() {
    let component_id = ComponentId::from("RiskEngine");

    let mut message = capnp::message::Builder::new_default();
    let builder = message.init_root::<identifiers_capnp::component_id::Builder>();
    component_id.to_capnp(builder);

    let mut bytes = Vec::new();
    capnp::serialize::write_message(&mut bytes, &message).unwrap();

    let reader =
        capnp::serialize::read_message(&mut &bytes[..], capnp::message::ReaderOptions::new())
            .unwrap();
    let root = reader
        .get_root::<identifiers_capnp::component_id::Reader>()
        .unwrap();
    let decoded = ComponentId::from_capnp(root).unwrap();

    assert_eq!(component_id, decoded);
}

#[rstest]
fn test_order_list_id_roundtrip() {
    let order_list_id = OrderListId::from("OL-001");

    let mut message = capnp::message::Builder::new_default();
    let builder = message.init_root::<identifiers_capnp::order_list_id::Builder>();
    order_list_id.to_capnp(builder);

    let mut bytes = Vec::new();
    capnp::serialize::write_message(&mut bytes, &message).unwrap();

    let reader =
        capnp::serialize::read_message(&mut &bytes[..], capnp::message::ReaderOptions::new())
            .unwrap();
    let root = reader
        .get_root::<identifiers_capnp::order_list_id::Reader>()
        .unwrap();
    let decoded = OrderListId::from_capnp(root).unwrap();

    assert_eq!(order_list_id, decoded);
}

#[rstest]
fn test_symbol_roundtrip() {
    let symbol = Symbol::from("AAPL");

    let mut message = capnp::message::Builder::new_default();
    let builder = message.init_root::<identifiers_capnp::symbol::Builder>();
    symbol.to_capnp(builder);

    let mut bytes = Vec::new();
    capnp::serialize::write_message(&mut bytes, &message).unwrap();

    let reader =
        capnp::serialize::read_message(&mut &bytes[..], capnp::message::ReaderOptions::new())
            .unwrap();
    let root = reader
        .get_root::<identifiers_capnp::symbol::Reader>()
        .unwrap();
    let decoded = Symbol::from_capnp(root).unwrap();

    assert_eq!(symbol, decoded);
}

#[rstest]
fn test_venue_roundtrip() {
    let venue = Venue::from("NASDAQ");

    let mut message = capnp::message::Builder::new_default();
    let builder = message.init_root::<identifiers_capnp::venue::Builder>();
    venue.to_capnp(builder);

    let mut bytes = Vec::new();
    capnp::serialize::write_message(&mut bytes, &message).unwrap();

    let reader =
        capnp::serialize::read_message(&mut &bytes[..], capnp::message::ReaderOptions::new())
            .unwrap();
    let root = reader
        .get_root::<identifiers_capnp::venue::Reader>()
        .unwrap();
    let decoded = Venue::from_capnp(root).unwrap();

    assert_eq!(venue, decoded);
}

#[rstest]
fn test_instrument_id_roundtrip() {
    let instrument_id = InstrumentId::from("AAPL.NASDAQ");

    let mut message = capnp::message::Builder::new_default();
    let builder = message.init_root::<identifiers_capnp::instrument_id::Builder>();
    instrument_id.to_capnp(builder);

    let mut bytes = Vec::new();
    capnp::serialize::write_message(&mut bytes, &message).unwrap();

    let reader =
        capnp::serialize::read_message(&mut &bytes[..], capnp::message::ReaderOptions::new())
            .unwrap();
    let root = reader
        .get_root::<identifiers_capnp::instrument_id::Reader>()
        .unwrap();
    let decoded = InstrumentId::from_capnp(root).unwrap();

    assert_eq!(instrument_id, decoded);
}
