// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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

use std::{cell::RefCell, rc::Rc};

use nautilus_backtest::data_client::BacktestDataClient;
use nautilus_common::{
    cache::Cache,
    clock::TestClock,
    messages::data::{DataCommand, SubscribeCommand, SubscribeOptionChain},
    msgbus::MessageBus,
};
use nautilus_core::{UUID4, UnixNanos};
use nautilus_data::{client::DataClientAdapter, engine::DataEngine};
use nautilus_model::{
    data::option_chain::StrikeRange,
    enums::OptionKind,
    identifiers::{ClientId, InstrumentId, OptionSeriesId, Symbol, TraderId, Venue},
    instruments::{CryptoOption, InstrumentAny},
    stubs::TestDefault,
    types::{Currency, Money, Price, Quantity},
};
use rstest::rstest;
use ustr::Ustr;

const EXPIRATION_NS: u64 = 1_704_067_200_000_000_000;

fn make_btc_option(strike: &str, kind: OptionKind) -> InstrumentAny {
    let kind_char = match kind {
        OptionKind::Call => "C",
        OptionKind::Put => "P",
    };
    let symbol_str = format!("BTC-20240101-{strike}-{kind_char}.DERIBIT");
    let raw_symbol_str = symbol_str.split('.').next().unwrap();
    InstrumentAny::CryptoOption(
        CryptoOption::builder()
            .instrument_id(InstrumentId::from(symbol_str.as_str()))
            .raw_symbol(Symbol::from(raw_symbol_str))
            .underlying(Currency::from("BTC"))
            .quote_currency(Currency::USD())
            .settlement_currency(Currency::from("BTC"))
            .is_inverse(false)
            .option_kind(kind)
            .strike_price(Price::from(strike))
            .activation_ns(UnixNanos::from(1_671_696_000_000_000_000u64))
            .expiration_ns(UnixNanos::from(EXPIRATION_NS))
            .price_precision(3)
            .size_precision(1)
            .price_increment(Price::from("0.001"))
            .size_increment(Quantity::from("0.1"))
            .multiplier(Quantity::from(1))
            .lot_size(Quantity::from(1))
            .max_quantity(Quantity::from("9000.0"))
            .min_quantity(Quantity::from("0.1"))
            .min_notional(Money::new(10.00, Currency::USD()))
            .ts_event(0.into())
            .ts_init(0.into())
            .build()
            .unwrap(),
    )
}

#[rstest]
fn test_atm_relative_subscription_unblocks_via_backtest_client() {
    let _ =
        MessageBus::new(TraderId::test_default(), UUID4::new(), None, None).register_message_bus();

    let clock = Rc::new(RefCell::new(TestClock::new()));
    let cache = Rc::new(RefCell::new(Cache::default()));

    let data_engine = Rc::new(RefCell::new(DataEngine::new(clock, cache.clone(), None)));
    DataEngine::register_msgbus_handlers(&data_engine);

    let client_id = ClientId::new("DERIBIT");
    let venue = Venue::new("DERIBIT");
    let backtest_client = BacktestDataClient::new(client_id, venue, cache.clone());
    let adapter = DataClientAdapter::new(
        client_id,
        Some(venue),
        true,
        true,
        Box::new(backtest_client),
    );
    data_engine
        .borrow_mut()
        .register_client(adapter, Some(venue));

    for strike in &["45000.000", "50000.000", "55000.000"] {
        let _ = cache
            .borrow_mut()
            .add_instrument(make_btc_option(strike, OptionKind::Call));
        let _ = cache
            .borrow_mut()
            .add_instrument(make_btc_option(strike, OptionKind::Put));
    }

    let series_id = OptionSeriesId::new(
        venue,
        Ustr::from("BTC"),
        Ustr::from("BTC"),
        UnixNanos::from(EXPIRATION_NS),
    );

    let cmd = DataCommand::Subscribe(SubscribeCommand::OptionChain(SubscribeOptionChain::new(
        series_id,
        StrikeRange::AtmRelative {
            strikes_above: 2,
            strikes_below: 2,
        },
        Some(1000),
        UUID4::new(),
        UnixNanos::default(),
        Some(client_id),
        Some(venue),
        None,
    )));

    data_engine.borrow_mut().execute(cmd);

    // Engine slow path called BacktestDataClient::request_forward_prices, which returns
    // Err. The engine fallback synchronously pops the pending request and creates the
    // manager (no response routing required). This pins the regression contract for
    // issue #3938 on the Rust path.
    let engine = data_engine.borrow();
    assert_eq!(engine.pending_option_chain_request_count(), 0);
    assert!(engine.has_option_chain_manager(&series_id));
}
