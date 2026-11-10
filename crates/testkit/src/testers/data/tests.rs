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

use std::{cell::RefCell, num::NonZeroUsize, rc::Rc};

use jiff::Timestamp;
use nautilus_common::{
    actor::DataActor,
    cache::Cache,
    clock::VirtualClock,
    messages::data::{DataCommand, RequestCommand},
    msgbus::{self, MessagingSwitchboard, stubs::get_typed_into_message_saving_handler},
    timer::TimeEvent,
};
use nautilus_core::{Params, UUID4, UnixNanos};
use nautilus_model::{
    data::{
        Bar, BarType, FundingRateUpdate, IndexPriceUpdate, InstrumentClose, InstrumentStatus,
        MarkPriceUpdate, OrderBookDelta, OrderBookDeltas, QuoteTick, TradeTick,
    },
    enums::{InstrumentCloseType, MarketStatusAction},
    identifiers::{ClientId, InstrumentId, Symbol, TraderId},
    instruments::{CurrencyPair, InstrumentAny},
    types::{Currency, Price, Quantity},
};
use rstest::*;
use rust_decimal::Decimal;

use super::*;

#[fixture]
fn config() -> DataTesterConfig {
    let client_id = ClientId::new("TEST");
    let instrument_ids = vec![
        InstrumentId::from("BTC-USDT.TEST"),
        InstrumentId::from("ETH-USDT.TEST"),
    ];
    let mut config = DataTesterConfig::new(client_id, instrument_ids);
    config.subscribe_quotes = true;
    config.subscribe_trades = true;
    config
}

#[rstest]
fn test_config_creation() {
    let client_id = ClientId::new("TEST");
    let instrument_ids = vec![InstrumentId::from("BTC-USDT.TEST")];
    let mut config = DataTesterConfig::new(client_id, instrument_ids.clone());
    config.subscribe_quotes = true;

    assert_eq!(config.client_id, Some(client_id));
    assert_eq!(config.instrument_ids, instrument_ids);
    assert!(config.subscribe_quotes);
    assert!(!config.subscribe_trades);
    assert!(config.log_data);
    assert_eq!(config.stats_interval_secs, 5);
}

#[rstest]
fn test_config_default() {
    let config = DataTesterConfig::default();

    assert_eq!(config.client_id, None);
    assert!(config.instrument_ids.is_empty());
    assert!(!config.subscribe_quotes);
    assert!(!config.subscribe_trades);
    assert!(!config.subscribe_bars);
    assert!(!config.request_instruments);
    assert!(!config.request_book_snapshot);
    assert!(!config.request_book_deltas);
    assert!(!config.request_trades);
    assert!(!config.request_bars);
    assert!(!config.request_funding_rates);
    assert!(config.can_unsubscribe);
    assert!(config.log_data);
    assert!(config.subscribe_params.is_none());
    assert!(config.request_params.is_none());
    assert_eq!(config.request_start, None);
    assert_eq!(config.request_end, None);
    assert_eq!(config.request_limit, None);
}

#[rstest]
#[case::defaults(None, None, None, "2026-10-31T23:00:00Z", "2026-10-25T00:00:00Z")]
#[case::bounded(
    Some("2026-10-29T01:02:03.123456789Z"),
    Some("2026-10-29T01:07:08.987654321Z"),
    Some(23),
    "2026-10-29T01:02:03.123456789Z",
    "2026-10-29T01:02:03.123456789Z"
)]
#[case::end_only(
    None,
    Some("2026-10-29T01:07:08Z"),
    Some(31),
    "2026-10-29T00:07:08Z",
    "2026-10-22T01:07:08Z"
)]
#[case::start_only(
    Some("2026-10-28T02:03:04Z"),
    None,
    Some(47),
    "2026-10-28T02:03:04Z",
    "2026-10-28T02:03:04Z"
)]
fn test_on_start_historical_request_bounds(
    #[case] start: Option<&str>,
    #[case] end: Option<&str>,
    #[case] limit: Option<usize>,
    #[case] expected_start: &str,
    #[case] expected_funding_start: &str,
) {
    let now: Timestamp = "2026-11-01T00:00:00Z".parse().unwrap();
    let clock = Rc::new(RefCell::new(VirtualClock::new()));
    clock.borrow_mut().set_time(UnixNanos::from(now));
    let cache = Rc::new(RefCell::new(Cache::default()));
    let instrument_id = InstrumentId::from("BTC-USDT.TEST");
    let client_id = ClientId::new("HISTORY");
    let bar_type = BarType::from("BTC-USDT.TEST-1-MINUTE-LAST-EXTERNAL");
    let start = start.map(|value| value.parse::<Timestamp>().unwrap());
    let end = end.map(|value| value.parse::<Timestamp>().unwrap());
    let limit = limit.map(|value| NonZeroUsize::new(value).unwrap());
    let config = DataTesterConfig::builder()
        .client_id(client_id)
        .instrument_ids(vec![instrument_id])
        .bar_types(vec![bar_type])
        .request_quotes(true)
        .request_trades(true)
        .request_bars(true)
        .request_funding_rates(true)
        .maybe_request_start(start)
        .maybe_request_end(end)
        .maybe_request_limit(limit)
        .stats_interval_secs(0)
        .build()
        .unwrap();
    let mut actor = DataTester::new(config);
    actor
        .core
        .register(TraderId::from("TESTER-001"), clock, cache)
        .unwrap();
    let (handler, saver) = get_typed_into_message_saving_handler::<DataCommand>(None);
    msgbus::register_data_command_endpoint(
        MessagingSwitchboard::data_engine_queue_execute(),
        handler,
    );

    actor.on_start().unwrap();

    let bounds: Vec<_> = saver
        .get_messages()
        .into_iter()
        .map(|command| match command {
            DataCommand::Request(RequestCommand::Quotes(request)) => {
                assert_eq!(request.instrument_ids, vec![instrument_id]);
                (
                    "quotes",
                    request.start,
                    request.end,
                    request.limit,
                    request.client_id,
                )
            }
            DataCommand::Request(RequestCommand::Trades(request)) => {
                assert_eq!(request.instrument_ids, vec![instrument_id]);
                (
                    "trades",
                    request.start,
                    request.end,
                    request.limit,
                    request.client_id,
                )
            }
            DataCommand::Request(RequestCommand::FundingRates(request)) => {
                assert_eq!(request.instrument_ids, vec![instrument_id]);
                (
                    "funding",
                    request.start,
                    request.end,
                    request.limit,
                    request.client_id,
                )
            }
            DataCommand::Request(RequestCommand::Bars(request)) => {
                assert_eq!(request.bar_types, vec![bar_type]);
                (
                    "bars",
                    request.start,
                    request.end,
                    request.limit,
                    request.client_id,
                )
            }
            other => panic!("Unexpected data command: {other:?}"),
        })
        .collect();

    let expected_start = Some(expected_start.parse::<Timestamp>().unwrap());
    let expected_funding_start = Some(expected_funding_start.parse::<Timestamp>().unwrap());
    assert_eq!(
        bounds,
        vec![
            ("quotes", expected_start, end, limit, Some(client_id)),
            ("trades", expected_start, end, limit, Some(client_id)),
            (
                "funding",
                expected_funding_start,
                end,
                limit,
                Some(client_id)
            ),
            ("bars", expected_start, end, limit, Some(client_id)),
        ],
    );
}

#[rstest]
fn test_config_with_params() {
    let client_id = ClientId::new("TEST");
    let instrument_ids = vec![InstrumentId::from("BTC-USDT.TEST")];

    let sub_params = Params::new([("key".to_string(), serde_json::json!("value"))].into());

    let req_params = Params::new([("limit".to_string(), serde_json::json!(100))].into());

    let mut config = DataTesterConfig::new(client_id, instrument_ids);
    config.subscribe_params = Some(sub_params.clone());
    config.request_params = Some(req_params.clone());

    assert_eq!(config.subscribe_params, Some(sub_params));
    assert_eq!(config.request_params, Some(req_params));
}

#[rstest]
fn test_actor_creation(config: DataTesterConfig) {
    let actor = DataTester::new(config);

    assert_eq!(actor.config.client_id, Some(ClientId::new("TEST")));
    assert_eq!(actor.config.instrument_ids.len(), 2);
}

#[rstest]
fn test_on_quote_with_logging_enabled(config: DataTesterConfig) {
    let mut actor = DataTester::new(config);

    let quote = QuoteTick::default();
    let result = actor.on_quote(&quote);

    assert!(result.is_ok());
}

#[rstest]
fn test_on_quote_with_logging_disabled(mut config: DataTesterConfig) {
    config.log_data = false;
    let mut actor = DataTester::new(config);

    let quote = QuoteTick::default();
    let result = actor.on_quote(&quote);

    assert!(result.is_ok());
}

#[rstest]
fn test_on_trade(config: DataTesterConfig) {
    let mut actor = DataTester::new(config);

    let trade = TradeTick::default();
    let result = actor.on_trade(&trade);

    assert!(result.is_ok());
}

#[rstest]
fn test_on_bar(config: DataTesterConfig) {
    let mut actor = DataTester::new(config);

    let bar = Bar::default();
    let result = actor.on_bar(&bar);

    assert!(result.is_ok());
}

#[rstest]
fn test_on_instrument(config: DataTesterConfig) {
    let mut actor = DataTester::new(config);

    let instrument_id = InstrumentId::from("BTC-USDT.TEST");
    let instrument = CurrencyPair::builder()
        .instrument_id(instrument_id)
        .raw_symbol(Symbol::from("BTC/USDT"))
        .base_currency(Currency::USD())
        .quote_currency(Currency::USD())
        .price_precision(4)
        .size_precision(3)
        .price_increment(Price::from("0.0001"))
        .size_increment(Quantity::from("0.001"))
        .ts_event(UnixNanos::default())
        .ts_init(UnixNanos::default())
        .build()
        .unwrap();
    let result = actor.on_instrument(&InstrumentAny::CurrencyPair(instrument));

    assert!(result.is_ok());
}

#[rstest]
fn test_on_book_deltas_without_managed_book(config: DataTesterConfig) {
    let mut actor = DataTester::new(config);

    let instrument_id = InstrumentId::from("BTC-USDT.TEST");
    let delta = OrderBookDelta::clear(instrument_id, 0, UnixNanos::default(), UnixNanos::default());
    let deltas = OrderBookDeltas::new(instrument_id, vec![delta]);
    let result = actor.on_book_deltas(&deltas);

    assert!(result.is_ok());
}

#[rstest]
fn test_on_mark_price(config: DataTesterConfig) {
    let mut actor = DataTester::new(config);

    let instrument_id = InstrumentId::from("BTC-USDT.TEST");
    let price = Price::from("50000.0");
    let mark_price = MarkPriceUpdate::new(
        instrument_id,
        price,
        UnixNanos::default(),
        UnixNanos::default(),
    );
    let result = actor.on_mark_price(&mark_price);

    assert!(result.is_ok());
}

#[rstest]
fn test_on_index_price(config: DataTesterConfig) {
    let mut actor = DataTester::new(config);

    let instrument_id = InstrumentId::from("BTC-USDT.TEST");
    let price = Price::from("50000.0");
    let index_price = IndexPriceUpdate::new(
        instrument_id,
        price,
        UnixNanos::default(),
        UnixNanos::default(),
    );
    let result = actor.on_index_price(&index_price);

    assert!(result.is_ok());
}

#[rstest]
fn test_on_funding_rate(config: DataTesterConfig) {
    let mut actor = DataTester::new(config);

    let instrument_id = InstrumentId::from("BTC-USDT.TEST");
    let funding_rate = FundingRateUpdate::new(
        instrument_id,
        Decimal::new(1, 4),
        None,
        None,
        UnixNanos::default(),
        UnixNanos::default(),
    );
    let result = actor.on_funding_rate(&funding_rate);

    assert!(result.is_ok());
}

#[rstest]
fn test_on_historical_funding_rates(config: DataTesterConfig) {
    let mut actor = DataTester::new(config);

    let instrument_id = InstrumentId::from("BTC-USDT.TEST");
    let rates = vec![
        FundingRateUpdate::new(
            instrument_id,
            Decimal::new(1, 4),
            None,
            None,
            UnixNanos::default(),
            UnixNanos::default(),
        ),
        FundingRateUpdate::new(
            instrument_id,
            Decimal::new(2, 4),
            None,
            None,
            UnixNanos::default(),
            UnixNanos::default(),
        ),
    ];
    let result = actor.on_historical_funding_rates(&rates);

    assert!(result.is_ok());
}

#[rstest]
fn test_config_request_funding_rates() {
    let client_id = ClientId::new("TEST");
    let instrument_ids = vec![InstrumentId::from("BTC-USDT.TEST")];
    let mut config = DataTesterConfig::new(client_id, instrument_ids);
    config.request_funding_rates = true;

    assert!(config.request_funding_rates);
}

#[rstest]
fn test_config_request_book_deltas() {
    let client_id = ClientId::new("TEST");
    let instrument_ids = vec![InstrumentId::from("BTC-USDT.TEST")];
    let mut config = DataTesterConfig::new(client_id, instrument_ids);
    config.request_book_deltas = true;

    assert!(config.request_book_deltas);
}

#[rstest]
fn test_on_instrument_status(config: DataTesterConfig) {
    let mut actor = DataTester::new(config);

    let instrument_id = InstrumentId::from("BTC-USDT.TEST");
    let status = InstrumentStatus::new(
        instrument_id,
        MarketStatusAction::Trading,
        UnixNanos::default(),
        UnixNanos::default(),
        None,
        None,
        None,
        None,
        None,
    );
    let result = actor.on_instrument_status(&status);

    assert!(result.is_ok());
}

#[rstest]
fn test_on_instrument_close(config: DataTesterConfig) {
    let mut actor = DataTester::new(config);

    let instrument_id = InstrumentId::from("BTC-USDT.TEST");
    let price = Price::from("50000.0");
    let close = InstrumentClose::new(
        instrument_id,
        price,
        InstrumentCloseType::EndOfSession,
        UnixNanos::default(),
        UnixNanos::default(),
    );
    let result = actor.on_instrument_close(&close);

    assert!(result.is_ok());
}

#[rstest]
fn test_on_time_event(config: DataTesterConfig) {
    let mut actor = DataTester::new(config);

    let event = TimeEvent::new(
        "TEST".into(),
        UUID4::default(),
        UnixNanos::default(),
        UnixNanos::default(),
    );
    let result = actor.on_time_event(&event);

    assert!(result.is_ok());
}

#[rstest]
fn test_config_with_all_subscriptions_enabled(mut config: DataTesterConfig) {
    config.subscribe_book_deltas = true;
    config.subscribe_book_at_interval = true;
    config.subscribe_bars = true;
    config.subscribe_mark_prices = true;
    config.subscribe_index_prices = true;
    config.subscribe_funding_rates = true;
    config.subscribe_instrument = true;
    config.subscribe_instrument_status = true;
    config.subscribe_instrument_close = true;
    config.subscribe_option_greeks = true;

    let actor = DataTester::new(config);

    assert!(actor.config.subscribe_book_deltas);
    assert!(actor.config.subscribe_book_at_interval);
    assert!(actor.config.subscribe_bars);
    assert!(actor.config.subscribe_mark_prices);
    assert!(actor.config.subscribe_index_prices);
    assert!(actor.config.subscribe_funding_rates);
    assert!(actor.config.subscribe_instrument);
    assert!(actor.config.subscribe_instrument_status);
    assert!(actor.config.subscribe_instrument_close);
    assert!(actor.config.subscribe_option_greeks);
}

#[rstest]
fn test_on_option_greeks(config: DataTesterConfig) {
    use nautilus_model::{
        data::{OptionGreekValues, option_chain::OptionGreeks},
        enums::GreeksConvention,
    };

    let mut actor = DataTester::new(config);

    let instrument_id = InstrumentId::from("BTC-USD-250328-92000-C.OKX");
    let greeks = OptionGreeks {
        instrument_id,
        convention: GreeksConvention::BlackScholes,
        greeks: OptionGreekValues {
            delta: 0.53,
            gamma: 0.00001,
            vega: 0.004,
            theta: -0.002,
            rho: 0.0,
        },
        mark_iv: Some(0.53),
        bid_iv: Some(0.52),
        ask_iv: Some(0.55),
        underlying_price: None,
        open_interest: None,
        ts_event: UnixNanos::default(),
        ts_init: UnixNanos::default(),
    };
    let result = actor.on_option_greeks(&greeks);

    assert!(result.is_ok());
}

#[rstest]
fn test_config_with_book_management(mut config: DataTesterConfig) {
    config.manage_book = true;
    config.book_levels_to_print = 5;

    let actor = DataTester::new(config);

    assert!(actor.config.manage_book);
    assert_eq!(actor.config.book_levels_to_print, 5);
    assert!(actor.books.is_empty());
}

#[rstest]
fn test_config_with_custom_stats_interval(mut config: DataTesterConfig) {
    config.stats_interval_secs = 10;

    let actor = DataTester::new(config);

    assert_eq!(actor.config.stats_interval_secs, 10);
}

#[rstest]
fn test_config_with_unsubscribe_disabled(mut config: DataTesterConfig) {
    config.can_unsubscribe = false;

    let actor = DataTester::new(config);

    assert!(!actor.config.can_unsubscribe);
}
