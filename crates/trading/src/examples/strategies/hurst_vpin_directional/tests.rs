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

use std::{cell::RefCell, collections::VecDeque, rc::Rc};

use nautilus_common::{
    actor::DataActor,
    cache::Cache,
    clock::{Clock, VirtualClock},
    config::ConfigError,
    messages::execution::TradingCommand,
    msgbus::{
        self, MessagingSwitchboard,
        stubs::{TypedIntoMessageSavingHandler, get_typed_into_message_saving_handler},
    },
};
use nautilus_core::{UUID4, UnixNanos};
use nautilus_model::{
    data::{Bar, BarSpecification, BarType, QuoteTick, TradeTick},
    enums::{AggregationSource, AggressorSide, BarAggregation, OmsType, OrderSide, PriceType},
    events::{PositionClosed, order::spec::OrderFilledSpec},
    identifiers::{ClientOrderId, InstrumentId, PositionId, StrategyId, Symbol, TradeId, TraderId},
    instruments::{CryptoPerpetual, Instrument, InstrumentAny},
    orders::Order,
    position::Position,
    types::{Currency, Price, Quantity},
};
use nautilus_portfolio::portfolio::Portfolio;
use rstest::rstest;
use rust_decimal_macros::dec;
use ustr::Ustr;

use super::{
    HurstVpinDirectional, HurstVpinDirectionalConfig, cache_database::FailNthAddOrderDatabase,
};
use crate::{
    examples::strategies::hurst_vpin_directional::config::MAX_HURST_VPIN_WINDOW,
    strategy::StrategyConfig,
};

fn pf_xbtusd() -> CryptoPerpetual {
    CryptoPerpetual::builder()
        .instrument_id(InstrumentId::from("PF_XBTUSD.KRAKEN"))
        .raw_symbol(Symbol::from("PF_XBTUSD"))
        .base_currency(Currency::BTC())
        .quote_currency(Currency::USD())
        .settlement_currency(Currency::USD())
        .is_inverse(false)
        .price_precision(1)
        .size_precision(4)
        .price_increment(Price::from("0.5"))
        .size_increment(Quantity::from("0.0001"))
        .margin_init(dec!(0.02))
        .margin_maint(dec!(0.01))
        .ts_event(0.into())
        .ts_init(0.into())
        .build()
        .unwrap()
}

fn bar_type(instrument_id: InstrumentId) -> BarType {
    BarType::new(
        instrument_id,
        BarSpecification::new(2_000_000, BarAggregation::Value, PriceType::Last),
        AggregationSource::External,
    )
}

fn create_strategy(instrument_id: InstrumentId) -> HurstVpinDirectional {
    let config = HurstVpinDirectionalConfig::builder()
        .instrument_id(instrument_id)
        .bar_type(bar_type(instrument_id))
        .trade_size(Quantity::from("0.01"))
        .build();
    HurstVpinDirectional::new(config)
}

fn create_strategy_with_short_windows(instrument_id: InstrumentId) -> HurstVpinDirectional {
    let config = HurstVpinDirectionalConfig::builder()
        .instrument_id(instrument_id)
        .bar_type(bar_type(instrument_id))
        .trade_size(Quantity::from("0.01"))
        .hurst_window(4)
        .hurst_lags(vec![2, 4])
        .vpin_window(1)
        .max_holding_secs(60)
        .build();
    HurstVpinDirectional::new(config)
}

fn create_strategy_with_windows(
    instrument_id: InstrumentId,
    hurst_window: usize,
    hurst_lags: Vec<usize>,
) -> HurstVpinDirectional {
    let config = HurstVpinDirectionalConfig::builder()
        .instrument_id(instrument_id)
        .bar_type(bar_type(instrument_id))
        .trade_size(Quantity::from("0.01"))
        .hurst_window(hurst_window)
        .hurst_lags(hurst_lags)
        .build();
    HurstVpinDirectional::new(config)
}

#[rstest]
#[case(1, 1)]
#[case(128, 50)]
#[case(MAX_HURST_VPIN_WINDOW, MAX_HURST_VPIN_WINDOW)]
fn test_config_accepts_supported_windows(#[case] hurst_window: usize, #[case] vpin_window: usize) {
    let instrument_id = InstrumentId::from("PF_XBTUSD.KRAKEN");
    let config = HurstVpinDirectionalConfig::builder()
        .instrument_id(instrument_id)
        .bar_type(bar_type(instrument_id))
        .trade_size(Quantity::from("0.01"))
        .hurst_window(hurst_window)
        .vpin_window(vpin_window)
        .build();

    assert!(config.validate().is_ok());
}

#[rstest]
#[case(0, 1, "hurst_window")]
#[case(1, 0, "vpin_window")]
#[case(MAX_HURST_VPIN_WINDOW + 1, 1, "hurst_window")]
#[case(1, MAX_HURST_VPIN_WINDOW + 1, "vpin_window")]
#[case(usize::MAX, 1, "hurst_window")]
#[case(1, usize::MAX, "vpin_window")]
fn test_config_rejects_invalid_windows(
    #[case] hurst_window: usize,
    #[case] vpin_window: usize,
    #[case] expected_field: &str,
) {
    let instrument_id = InstrumentId::from("PF_XBTUSD.KRAKEN");
    let config = HurstVpinDirectionalConfig::builder()
        .instrument_id(instrument_id)
        .bar_type(bar_type(instrument_id))
        .trade_size(Quantity::from("0.01"))
        .hurst_window(hurst_window)
        .vpin_window(vpin_window)
        .build();

    let err = config
        .validate()
        .expect_err("invalid window must be rejected");

    assert!(matches!(err, ConfigError::Range { field, .. } if field == expected_field));
}

#[rstest]
fn test_strategy_new_checked_returns_invalid_window() {
    let instrument_id = InstrumentId::from("PF_XBTUSD.KRAKEN");
    let config = HurstVpinDirectionalConfig::builder()
        .instrument_id(instrument_id)
        .bar_type(bar_type(instrument_id))
        .trade_size(Quantity::from("0.01"))
        .hurst_window(0)
        .build();

    let result = HurstVpinDirectional::new_checked(config);

    assert!(matches!(
        result,
        Err(ConfigError::Range { field, .. }) if field == "hurst_window"
    ));
}

#[rstest]
fn test_strategy_new_checked_returns_invalid_order_id_tag() {
    let instrument_id = InstrumentId::from("PF_XBTUSD.KRAKEN");
    let config = HurstVpinDirectionalConfig::builder()
        .base(StrategyConfig {
            strategy_id: Some(StrategyId::from("HURST_VPIN-001")),
            order_id_tag: Some("T01€".to_string()),
            ..Default::default()
        })
        .instrument_id(instrument_id)
        .bar_type(bar_type(instrument_id))
        .trade_size(Quantity::from("0.01"))
        .build();

    let result = HurstVpinDirectional::new_checked(config);

    assert!(matches!(
        result,
        Err(ConfigError::InvalidValue { field, .. }) if field == "order_id_tag"
    ));
}

#[rstest]
fn test_strategy_allocates_maximum_supported_windows() {
    let instrument_id = InstrumentId::from("PF_XBTUSD.KRAKEN");
    let config = HurstVpinDirectionalConfig::builder()
        .instrument_id(instrument_id)
        .bar_type(bar_type(instrument_id))
        .trade_size(Quantity::from("0.01"))
        .hurst_window(MAX_HURST_VPIN_WINDOW)
        .vpin_window(MAX_HURST_VPIN_WINDOW)
        .build();

    let strategy = HurstVpinDirectional::new_checked(config).unwrap();

    assert!(strategy.returns.capacity() >= MAX_HURST_VPIN_WINDOW);
    assert!(strategy.abs_imbalances.capacity() >= MAX_HURST_VPIN_WINDOW);
    assert!(strategy.signed_imbalances.capacity() >= MAX_HURST_VPIN_WINDOW);
}

fn register_strategy(strategy: &mut HurstVpinDirectional) {
    let cache = Rc::new(RefCell::new(Cache::default()));
    register_strategy_with_cache(strategy, cache);
}

fn register_strategy_with_cache(strategy: &mut HurstVpinDirectional, cache: Rc<RefCell<Cache>>) {
    let trader_id = TraderId::from("TESTER-001");
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));

    let portfolio = Rc::new(RefCell::new(Portfolio::new(
        clock.clone(),
        cache.clone(),
        None,
    )));
    strategy
        .core
        .register(trader_id, clock, cache, portfolio)
        .unwrap();
}

fn capture_risk_commands() -> TypedIntoMessageSavingHandler<TradingCommand> {
    let (handler, messages): (_, TypedIntoMessageSavingHandler<TradingCommand>) =
        get_typed_into_message_saving_handler(Some(Ustr::from("RiskEngine.queue_execute")));
    msgbus::register_trading_command_endpoint(
        MessagingSwitchboard::risk_engine_queue_execute(),
        handler,
    );
    messages
}

fn submitted_order_ids(
    messages: &TypedIntoMessageSavingHandler<TradingCommand>,
) -> Vec<ClientOrderId> {
    messages
        .get_messages()
        .iter()
        .map(|message| match message {
            TradingCommand::SubmitOrder(command) => command.client_order_id,
            other => panic!("expected SubmitOrder command, was {other:?}"),
        })
        .collect()
}

fn trade(instrument_id: InstrumentId, size: &str, side: AggressorSide, ts: u64) -> TradeTick {
    TradeTick::new(
        instrument_id,
        Price::from("30000.0"),
        Quantity::from(size),
        side,
        TradeId::from("T-1"),
        ts.into(),
        ts.into(),
    )
}

fn bar(bar_type: BarType, close: &str, ts: u64) -> Bar {
    Bar::new(
        bar_type,
        Price::from(close),
        Price::from(close),
        Price::from(close),
        Price::from(close),
        Quantity::from("1"),
        ts.into(),
        ts.into(),
    )
}

fn quote(instrument_id: InstrumentId, ts: u64) -> QuoteTick {
    QuoteTick::new(
        instrument_id,
        Price::from("30000.0"),
        Price::from("30000.5"),
        Quantity::from("1"),
        Quantity::from("1"),
        ts.into(),
        ts.into(),
    )
}

fn cache_open_position(
    cache: &Rc<RefCell<Cache>>,
    strategy: &HurstVpinDirectional,
    position_id: &str,
    ts_opened: u64,
) -> Position {
    let instrument = InstrumentAny::CryptoPerpetual(pf_xbtusd());
    let fill = OrderFilledSpec::builder()
        .trader_id(TraderId::from("TESTER-001"))
        .strategy_id(strategy.core.strategy_id().unwrap())
        .instrument_id(instrument.id())
        .position_id(PositionId::from(position_id))
        .last_qty(Quantity::from("0.01"))
        .last_px(Price::from("30000.0"))
        .ts_event(ts_opened.into())
        .ts_init(ts_opened.into())
        .build();
    let position = Position::new(&instrument, fill);
    let mut cache = cache.borrow_mut();
    cache.add_instrument(instrument).unwrap();
    cache.add_position(&position, OmsType::Hedging).unwrap();
    position
}

fn set_ready_signals(strategy: &mut HurstVpinDirectional) {
    strategy.returns = VecDeque::from([0.01, -0.01, 0.02, -0.02]);
    strategy.abs_imbalances = VecDeque::from([0.4]);
    strategy.signed_imbalances = VecDeque::from([0.5]);
    strategy.hurst = Some(0.6);
    strategy.vpin = Some(0.4);
    strategy.signed_vpin = Some(0.5);
}

fn process_constant_bars(strategy: &mut HurstVpinDirectional) {
    let bar_type = strategy.config.bar_type;
    for ts in 1..=5 {
        strategy
            .on_trade(&trade(
                strategy.config.instrument_id,
                "1.0",
                AggressorSide::Buy,
                ts,
            ))
            .unwrap();
        strategy.on_bar(&bar(bar_type, "30000.0", ts)).unwrap();
    }
}

#[rstest]
fn test_new_initializes_clean_state() {
    let strategy = create_strategy(InstrumentId::from("PF_XBTUSD.KRAKEN"));

    assert!(strategy.hurst.is_none());
    assert!(strategy.vpin.is_none());
    assert!(strategy.signed_vpin.is_none());
    assert!(strategy.last_close.is_none());
    assert_eq!(strategy.bucket_buy_volume, 0.0);
    assert_eq!(strategy.bucket_sell_volume, 0.0);
    assert!(!strategy.exit_cooldown);
    assert!(strategy.entry_order_id.is_none());
    assert!(strategy.exit_order_ids.is_empty());
    assert!(strategy.position_opened_ns.is_none());
}

#[rstest]
fn test_config_defaults() {
    let instrument_id = InstrumentId::from("PF_XBTUSD.KRAKEN");
    let strategy = create_strategy(instrument_id);
    let config = &strategy.config;

    assert_eq!(config.hurst_window, 128);
    assert_eq!(config.hurst_lags, vec![4, 8, 16, 32]);
    assert_eq!(config.hurst_enter, 0.55);
    assert_eq!(config.hurst_exit, 0.50);
    assert_eq!(config.vpin_window, 50);
    assert_eq!(config.vpin_threshold, 0.30);
    assert_eq!(config.max_holding_secs, 3600);
    assert_eq!(
        strategy.core.config.strategy_id,
        Some(StrategyId::from("HURST_VPIN-001")),
    );
}

#[rstest]
fn test_buyer_aggressor_adds_to_buy_volume() {
    let mut strategy = create_strategy(InstrumentId::from("PF_XBTUSD.KRAKEN"));

    strategy
        .on_trade(&trade(
            strategy.config.instrument_id,
            "5.0",
            AggressorSide::Buy,
            1,
        ))
        .unwrap();

    assert_eq!(strategy.bucket_buy_volume, 5.0);
    assert_eq!(strategy.bucket_sell_volume, 0.0);
}

#[rstest]
fn test_seller_aggressor_adds_to_sell_volume() {
    let mut strategy = create_strategy(InstrumentId::from("PF_XBTUSD.KRAKEN"));

    strategy
        .on_trade(&trade(
            strategy.config.instrument_id,
            "7.0",
            AggressorSide::Sell,
            1,
        ))
        .unwrap();

    assert_eq!(strategy.bucket_buy_volume, 0.0);
    assert_eq!(strategy.bucket_sell_volume, 7.0);
}

#[rstest]
fn test_no_aggressor_is_ignored() {
    let mut strategy = create_strategy(InstrumentId::from("PF_XBTUSD.KRAKEN"));

    strategy
        .on_trade(&trade(
            strategy.config.instrument_id,
            "3.0",
            AggressorSide::NoAggressor,
            1,
        ))
        .unwrap();

    assert_eq!(strategy.bucket_buy_volume, 0.0);
    assert_eq!(strategy.bucket_sell_volume, 0.0);
}

#[rstest]
fn test_first_bar_records_close_but_not_return() {
    let mut strategy = create_strategy(InstrumentId::from("PF_XBTUSD.KRAKEN"));
    register_strategy(&mut strategy);
    let cache = Rc::new(RefCell::new(Cache::default()));
    cache
        .borrow_mut()
        .add_instrument(InstrumentAny::CryptoPerpetual(pf_xbtusd()))
        .unwrap();

    let bt = bar_type(strategy.config.instrument_id);
    strategy.on_bar(&bar(bt, "30000.0", 0)).unwrap();

    assert_eq!(strategy.last_close, Some(30000.0));
    assert_eq!(strategy.returns.len(), 0);
}

#[rstest]
fn test_second_bar_appends_log_return() {
    let mut strategy = create_strategy(InstrumentId::from("PF_XBTUSD.KRAKEN"));
    register_strategy(&mut strategy);

    let bt = bar_type(strategy.config.instrument_id);
    strategy.on_bar(&bar(bt, "30000.0", 0)).unwrap();
    strategy.on_bar(&bar(bt, "33000.0", 1)).unwrap();

    assert_eq!(strategy.returns.len(), 1);
    let expected = (33000.0_f64 / 30000.0_f64).ln();
    assert!((strategy.returns[0] - expected).abs() < 1e-9);
}

#[rstest]
fn test_zero_volume_bar_does_not_record_imbalance() {
    let mut strategy = create_strategy(InstrumentId::from("PF_XBTUSD.KRAKEN"));
    register_strategy(&mut strategy);

    let bt = bar_type(strategy.config.instrument_id);
    strategy.on_bar(&bar(bt, "30000.0", 0)).unwrap();
    strategy.on_bar(&bar(bt, "30010.0", 1)).unwrap();

    assert!(strategy.abs_imbalances.is_empty());
    assert!(strategy.signed_imbalances.is_empty());
}

#[rstest]
fn test_bar_finalizes_bucket_and_resets_accumulators() {
    let mut strategy = create_strategy(InstrumentId::from("PF_XBTUSD.KRAKEN"));
    register_strategy(&mut strategy);

    let bt = bar_type(strategy.config.instrument_id);
    strategy.on_bar(&bar(bt, "30000.0", 0)).unwrap();
    strategy
        .on_trade(&trade(
            strategy.config.instrument_id,
            "7.0",
            AggressorSide::Buy,
            1,
        ))
        .unwrap();
    strategy
        .on_trade(&trade(
            strategy.config.instrument_id,
            "3.0",
            AggressorSide::Sell,
            2,
        ))
        .unwrap();
    strategy.on_bar(&bar(bt, "30010.0", 3)).unwrap();

    assert_eq!(strategy.abs_imbalances.len(), 1);
    assert!((strategy.abs_imbalances[0] - 0.4).abs() < 1e-9);
    assert_eq!(strategy.signed_imbalances.len(), 1);
    assert!((strategy.signed_imbalances[0] - 0.4).abs() < 1e-9);
    assert_eq!(strategy.bucket_buy_volume, 0.0);
    assert_eq!(strategy.bucket_sell_volume, 0.0);
}

#[rstest]
fn test_bar_clears_exit_cooldown() {
    let mut strategy = create_strategy(InstrumentId::from("PF_XBTUSD.KRAKEN"));
    register_strategy(&mut strategy);
    strategy.exit_cooldown = true;

    let bt = bar_type(strategy.config.instrument_id);
    strategy.on_bar(&bar(bt, "30000.0", 0)).unwrap();

    assert!(!strategy.exit_cooldown);
}

#[rstest]
fn test_hurst_returns_none_when_insufficient_returns() {
    let mut strategy = create_strategy(InstrumentId::from("PF_XBTUSD.KRAKEN"));
    // Fewer than hurst_window (128)
    for value in [0.01, -0.01, 0.01] {
        strategy.returns.push_back(value);
    }
    assert!(strategy.estimate_hurst().is_none());
}

#[rstest]
fn test_hurst_mean_reverting_series_below_half() {
    let mut strategy = create_strategy_with_windows(
        InstrumentId::from("PF_XBTUSD.KRAKEN"),
        128,
        vec![4, 8, 16, 32],
    );

    for i in 0..strategy.config.hurst_window {
        strategy
            .returns
            .push_back(if i % 2 == 0 { 0.01 } else { -0.01 });
    }

    let h = strategy.estimate_hurst().unwrap();
    assert!(h < 0.30, "expected mean-reverting Hurst < 0.30, was {h}");
}

#[rstest]
fn test_hurst_persistent_series_above_half() {
    // AR(1) with positive coefficient produces positively autocorrelated returns
    let mut strategy = create_strategy_with_windows(
        InstrumentId::from("PF_XBTUSD.KRAKEN"),
        128,
        vec![4, 8, 16, 32],
    );
    let mut state: u64 = 0x1234_5678_9abc_def0;
    let mut prev = 0.0_f64;

    for _ in 0..strategy.config.hurst_window {
        // Seeded xorshift for deterministic noise
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        let u = (state as f64 / u64::MAX as f64 - 0.5) * 0.002;
        let value = 0.9 * prev + u;
        strategy.returns.push_back(value);
        prev = value;
    }

    let h = strategy.estimate_hurst().unwrap();
    assert!(h > 0.70, "expected persistent Hurst > 0.70, was {h}");
}

#[rstest]
fn test_rolling_mean_on_empty_returns_none() {
    let empty: VecDeque<f64> = VecDeque::new();
    assert!(HurstVpinDirectional::rolling_mean(&empty).is_none());
}

#[rstest]
fn test_rolling_mean_basic_average() {
    let mut values: VecDeque<f64> = VecDeque::new();
    values.push_back(1.0);
    values.push_back(2.0);
    values.push_back(3.0);
    assert_eq!(HurstVpinDirectional::rolling_mean(&values), Some(2.0));
}

#[rstest]
fn test_push_bounded_respects_capacity() {
    let mut values: VecDeque<f64> = VecDeque::new();
    HurstVpinDirectional::push_bounded(&mut values, 3, 1.0);
    HurstVpinDirectional::push_bounded(&mut values, 3, 2.0);
    HurstVpinDirectional::push_bounded(&mut values, 3, 3.0);
    HurstVpinDirectional::push_bounded(&mut values, 3, 4.0);

    assert_eq!(values.len(), 3);
    assert_eq!(values[0], 2.0);
    assert_eq!(values[2], 4.0);
}

#[rstest]
fn test_signals_ready_false_during_warmup() {
    let strategy = create_strategy(InstrumentId::from("PF_XBTUSD.KRAKEN"));
    assert!(!strategy.signals_ready());
}

#[rstest]
fn test_on_start_rejects_mismatched_bar_type() {
    let instrument_id = InstrumentId::from("PF_XBTUSD.KRAKEN");
    let other_id = InstrumentId::from("OTHER.KRAKEN");
    let config = HurstVpinDirectionalConfig::builder()
        .instrument_id(instrument_id)
        .bar_type(bar_type(other_id))
        .trade_size(Quantity::from("0.01"))
        .build();
    let mut strategy = HurstVpinDirectional::new(config);
    register_strategy(&mut strategy);

    let err = strategy.on_start().unwrap_err();
    assert!(
        err.to_string().contains("does not match traded instrument"),
        "unexpected error: {err}"
    );
}

#[rstest]
fn test_on_reset_clears_all_state() {
    let mut strategy = create_strategy(InstrumentId::from("PF_XBTUSD.KRAKEN"));
    register_strategy(&mut strategy);

    strategy.returns.push_back(0.01);
    strategy.abs_imbalances.push_back(0.4);
    strategy.signed_imbalances.push_back(-0.3);
    strategy.last_close = Some(30000.0);
    strategy.bucket_buy_volume = 1.0;
    strategy.bucket_sell_volume = 2.0;
    strategy.hurst = Some(0.6);
    strategy.vpin = Some(0.4);
    strategy.signed_vpin = Some(0.4);
    strategy.position_opened_ns = Some(UnixNanos::new(12_345));
    strategy.exit_cooldown = true;
    strategy.entry_order_id = Some(ClientOrderId::from("O-1"));
    strategy.exit_order_ids.insert(ClientOrderId::from("O-2"));

    strategy.on_reset().unwrap();

    assert!(strategy.returns.is_empty());
    assert!(strategy.abs_imbalances.is_empty());
    assert!(strategy.signed_imbalances.is_empty());
    assert!(strategy.last_close.is_none());
    assert_eq!(strategy.bucket_buy_volume, 0.0);
    assert_eq!(strategy.bucket_sell_volume, 0.0);
    assert!(strategy.hurst.is_none());
    assert!(strategy.vpin.is_none());
    assert!(strategy.signed_vpin.is_none());
    assert!(strategy.position_opened_ns.is_none());
    assert!(!strategy.exit_cooldown);
    assert!(strategy.entry_order_id.is_none());
    assert!(strategy.exit_order_ids.is_empty());
}

#[rstest]
fn test_entry_submission_error_releases_latch_and_allows_retry() {
    let instrument_id = InstrumentId::from("PF_XBTUSD.KRAKEN");
    let (database, database_control) = FailNthAddOrderDatabase::create();
    let cache = Rc::new(RefCell::new(Cache::new(None, Some(Box::new(database)))));
    let mut strategy = create_strategy_with_short_windows(instrument_id);
    register_strategy_with_cache(&mut strategy, cache.clone());
    set_ready_signals(&mut strategy);
    let risk_commands = capture_risk_commands();
    database_control.set_fail_add_order_on(Some(1));

    let error = strategy.on_quote(&quote(instrument_id, 1)).unwrap_err();

    assert_eq!(error.to_string(), "test add order failure");
    assert!(strategy.entry_order_id.is_none());
    assert!(submitted_order_ids(&risk_commands).is_empty());

    database_control.set_fail_add_order_on(None);
    strategy.on_quote(&quote(instrument_id, 2)).unwrap();

    let retry_id = strategy.entry_order_id.unwrap();
    assert_eq!(submitted_order_ids(&risk_commands), vec![retry_id]);
    let cache = cache.borrow();
    let order = cache.order(&retry_id).unwrap();
    assert_eq!(order.order_side(), OrderSide::Buy);
    assert_eq!(order.quantity(), Quantity::from("0.01"));
}

#[rstest]
fn test_close_submission_error_releases_failed_latch_and_allows_retry() {
    let instrument_id = InstrumentId::from("PF_XBTUSD.KRAKEN");
    let (database, database_control) = FailNthAddOrderDatabase::create();
    let cache = Rc::new(RefCell::new(Cache::new(None, Some(Box::new(database)))));
    let mut strategy = create_strategy_with_short_windows(instrument_id);
    register_strategy_with_cache(&mut strategy, cache.clone());
    cache_open_position(&cache, &strategy, "P-RETRY-1", 0);
    strategy.position_opened_ns = Some(UnixNanos::from(0));
    set_ready_signals(&mut strategy);
    let risk_commands = capture_risk_commands();
    database_control.set_fail_add_order_on(Some(1));

    let error = strategy
        .on_quote(&quote(instrument_id, 61_000_000_000))
        .unwrap_err();

    assert_eq!(error.to_string(), "test add order failure");
    assert!(strategy.exit_order_ids.is_empty());
    assert!(strategy.exit_cooldown);
    assert!(submitted_order_ids(&risk_commands).is_empty());

    database_control.set_fail_add_order_on(None);
    strategy
        .on_quote(&quote(instrument_id, 62_000_000_000))
        .unwrap();

    let submitted_ids = submitted_order_ids(&risk_commands);
    assert_eq!(submitted_ids.len(), 1);
    assert!(strategy.exit_order_ids.contains(&submitted_ids[0]));
    assert_eq!(strategy.exit_order_ids.len(), 1);
    let cache = cache.borrow();
    let order = cache.order(&submitted_ids[0]).unwrap();
    assert_eq!(order.order_side(), OrderSide::Sell);
    assert_eq!(order.quantity(), Quantity::from("0.01"));
    assert!(order.is_reduce_only());
}

#[rstest]
fn test_close_submission_error_preserves_successful_exit_ids() {
    let instrument_id = InstrumentId::from("PF_XBTUSD.KRAKEN");
    let (database, database_control) = FailNthAddOrderDatabase::create();
    let cache = Rc::new(RefCell::new(Cache::new(None, Some(Box::new(database)))));
    let mut strategy = create_strategy_with_short_windows(instrument_id);
    register_strategy_with_cache(&mut strategy, cache.clone());
    cache_open_position(&cache, &strategy, "P-PARTIAL-1", 0);
    cache_open_position(&cache, &strategy, "P-PARTIAL-2", 0);
    strategy.position_opened_ns = Some(UnixNanos::from(0));
    set_ready_signals(&mut strategy);
    let risk_commands = capture_risk_commands();
    database_control.set_fail_add_order_on(Some(2));

    let error = strategy
        .on_quote(&quote(instrument_id, 61_000_000_000))
        .unwrap_err();

    let submitted_ids = submitted_order_ids(&risk_commands);
    assert_eq!(error.to_string(), "test add order failure");
    assert_eq!(submitted_ids.len(), 1);
    assert_eq!(strategy.exit_order_ids.len(), 1);
    assert!(strategy.exit_order_ids.contains(&submitted_ids[0]));
    assert!(strategy.exit_cooldown);

    let successful_position_id = {
        let messages = risk_commands.get_messages();

        let TradingCommand::SubmitOrder(command) = &messages[0] else {
            panic!("expected SubmitOrder command");
        };

        command.position_id.unwrap()
    };

    let closing_fill = OrderFilledSpec::builder()
        .trader_id(TraderId::from("TESTER-001"))
        .strategy_id(strategy.core.strategy_id().unwrap())
        .instrument_id(instrument_id)
        .client_order_id(submitted_ids[0])
        .position_id(successful_position_id)
        .trade_id(TradeId::from("T-CLOSE-PARTIAL"))
        .order_side(OrderSide::Sell)
        .last_qty(Quantity::from("0.01"))
        .last_px(Price::from("30000.0"))
        .ts_event(61_000_000_000.into())
        .ts_init(61_000_000_000.into())
        .build();
    let closed_position = cache
        .borrow_mut()
        .update_position_from_fill(successful_position_id, &closing_fill)
        .unwrap();
    let closed_event = PositionClosed::create(
        &closed_position,
        &closing_fill,
        UUID4::new(),
        61_000_000_000.into(),
    );
    crate::strategy::Strategy::on_position_closed(&mut strategy, closed_event);
    strategy.exit_order_ids.remove(&submitted_ids[0]);
    database_control.set_fail_add_order_on(None);
    strategy
        .on_quote(&quote(instrument_id, 62_000_000_000))
        .unwrap();

    let retry_ids = submitted_order_ids(&risk_commands);
    assert_eq!(strategy.position_opened_ns, Some(UnixNanos::from(0)));
    assert_eq!(retry_ids.len(), 2);
    assert_eq!(strategy.exit_order_ids.len(), 1);
    assert!(!strategy.exit_order_ids.contains(&submitted_ids[0]));
    assert!(strategy.exit_order_ids.contains(&retry_ids[1]));
    let cache = cache.borrow();
    let order = cache.order(&retry_ids[1]).unwrap();
    assert_eq!(order.order_side(), OrderSide::Sell);
    assert!(order.is_reduce_only());
}

#[rstest]
fn test_on_start_hydrates_cached_position_and_enforces_timeout() {
    let instrument_id = InstrumentId::from("PF_XBTUSD.KRAKEN");
    let cache = Rc::new(RefCell::new(Cache::default()));
    let mut strategy = create_strategy_with_short_windows(instrument_id);
    register_strategy_with_cache(&mut strategy, cache.clone());
    cache_open_position(&cache, &strategy, "P-INHERITED-1", 0);
    let risk_commands = capture_risk_commands();

    strategy.on_start().unwrap();
    set_ready_signals(&mut strategy);
    strategy
        .on_quote(&quote(instrument_id, 61_000_000_000))
        .unwrap();

    assert_eq!(strategy.position_opened_ns, Some(UnixNanos::from(0)));
    let submitted_ids = submitted_order_ids(&risk_commands);
    assert_eq!(submitted_ids.len(), 1);
    let cache = cache.borrow();
    let order = cache.order(&submitted_ids[0]).unwrap();
    assert_eq!(order.order_side(), OrderSide::Sell);
    assert!(order.is_reduce_only());
}

#[rstest]
fn test_on_start_uses_oldest_cached_position_opened_ns() {
    let instrument_id = InstrumentId::from("PF_XBTUSD.KRAKEN");
    let cache = Rc::new(RefCell::new(Cache::default()));
    let mut strategy = create_strategy_with_short_windows(instrument_id);
    register_strategy_with_cache(&mut strategy, cache.clone());
    cache_open_position(&cache, &strategy, "P-INHERITED-NEW", 20_000_000_000);
    cache_open_position(&cache, &strategy, "P-INHERITED-OLD", 10_000_000_000);

    strategy.on_start().unwrap();

    assert_eq!(
        strategy.position_opened_ns,
        Some(UnixNanos::from(10_000_000_000))
    );
}

#[rstest]
fn test_holding_timeout_runs_without_hurst_estimate() {
    let instrument_id = InstrumentId::from("PF_XBTUSD.KRAKEN");
    let cache = Rc::new(RefCell::new(Cache::default()));
    let mut strategy = create_strategy_with_short_windows(instrument_id);
    register_strategy_with_cache(&mut strategy, cache.clone());
    cache_open_position(&cache, &strategy, "P-TIMEOUT-1", 0);
    strategy.position_opened_ns = Some(UnixNanos::from(0));
    let risk_commands = capture_risk_commands();

    process_constant_bars(&mut strategy);
    strategy
        .on_quote(&quote(instrument_id, 61_000_000_000))
        .unwrap();

    assert!(strategy.hurst.is_none());
    assert_eq!(strategy.vpin, Some(1.0));
    assert_eq!(strategy.signed_vpin, Some(1.0));
    assert!(!strategy.signals_ready());
    let submitted_ids = submitted_order_ids(&risk_commands);
    assert_eq!(submitted_ids.len(), 1);
    let cache = cache.borrow();
    let order = cache.order(&submitted_ids[0]).unwrap();
    assert_eq!(order.order_side(), OrderSide::Sell);
    assert!(order.is_reduce_only());
}

#[rstest]
fn test_flat_quote_requires_ready_signals() {
    let instrument_id = InstrumentId::from("PF_XBTUSD.KRAKEN");
    let cache = Rc::new(RefCell::new(Cache::default()));
    let mut strategy = create_strategy_with_short_windows(instrument_id);
    register_strategy_with_cache(&mut strategy, cache);
    strategy.hurst = Some(0.6);
    strategy.vpin = Some(0.4);
    strategy.signed_vpin = Some(0.5);
    let risk_commands = capture_risk_commands();

    strategy.on_quote(&quote(instrument_id, 1)).unwrap();

    assert!(!strategy.signals_ready());
    assert!(strategy.entry_order_id.is_none());
    assert!(submitted_order_ids(&risk_commands).is_empty());
}
