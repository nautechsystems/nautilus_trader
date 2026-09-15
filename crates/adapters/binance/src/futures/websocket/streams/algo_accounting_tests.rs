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

// Regression coverage through the native order, execution engine, and position models.
mod algo_accounting {
    use std::{cell::RefCell, rc::Rc};

    use nautilus_common::{cache::Cache, clock::TestClock};
    use nautilus_execution::engine::ExecutionEngine;
    use nautilus_model::{
        accounts::{AccountAny, MarginAccount},
        enums::{OmsType, OrderStatus, OrderType},
        events::AccountState,
        orders::{Order, builder::OrderTestBuilder, stubs::TestOrderEventStubs},
        types::AccountBalance,
    };

    use super::*;
    use crate::{
        common::parse::parse_usdm_instrument, futures::http::models::BinanceFuturesUsdExchangeInfo,
    };

    // Original regression quantities: 0.010 BTC SELL, either fully filled or 0.004
    // partially filled. Extend it with a missing TRIGGERED message and duplicates.
    #[rstest]
    #[case::finished_then_trade_without_trigger(true, false)]
    #[case::trade_then_finished_without_trigger(false, false)]
    #[case::finished_then_trade_after_trigger(true, true)]
    #[case::trade_then_finished_after_trigger(false, true)]
    fn test_algo_finished_trade_accounting(
        #[case] finished_first: bool,
        #[case] trigger_first: bool,
        #[values("0.010", "0.004")] filled: &str,
        #[values(false, true)] split_fills: bool,
    ) {
        assert_algo_accounting(
            finished_first,
            trigger_first,
            filled,
            split_fills,
            false,
            None,
            None,
        );
    }

    #[rstest]
    fn test_algo_partial_cancel_before_late_trade(
        #[values(false, true)] finished_first: bool,
        #[values(false, true)] split_fills: bool,
    ) {
        assert_algo_accounting(finished_first, true, "0.004", split_fills, true, None, None);
    }

    #[rstest]
    fn test_algo_trade_lite_preserves_authoritative_fees(
        #[values(false, true)] lite_first: bool,
        #[values(false, true)] finished_first: bool,
        #[values("0.010", "0.004")] filled: &str,
        #[values(false, true)] split_fills: bool,
        #[values(false, true)] cancel_before_trade: bool,
    ) {
        assert_algo_accounting(
            finished_first,
            false,
            filled,
            split_fills,
            cancel_before_trade,
            Some(lite_first),
            None,
        );
    }

    #[rstest]
    fn test_evicted_algo_identity_reconciles_late_fills_to_original_order(
        #[values(false, true)] known_actual_id: bool,
        #[values("0.010", "0.004")] filled: &str,
        #[values(false, true)] split_fills: bool,
        #[values(false, true)] use_trade_lite: bool,
    ) {
        assert_algo_accounting(
            true,
            false,
            filled,
            split_fills,
            false,
            use_trade_lite.then_some(true),
            Some(known_actual_id),
        );
    }

    fn assert_algo_accounting(
        finished_first: bool,
        trigger_first: bool,
        filled: &str,
        split_fills: bool,
        cancel_before_trade: bool,
        trade_lite_first: Option<bool>,
        evicted_actual_id: Option<bool>,
    ) {
        let clock = get_atomic_clock_realtime();
        let (emitter, mut rx) = create_test_emitter(clock);
        let http = create_test_http_client(clock);
        let client_id = ClientOrderId::from("TEST");
        let instrument_id = InstrumentId::from("BTCUSDT-PERP.BINANCE");
        let account_id = AccountId::from("BINANCE-001");
        let mut algo: serde_json::Value = serde_json::from_str(&load_fixture_string(
            "futures/user_data_json/algo_update_new.json",
        ))
        .unwrap();
        algo["o"]["caid"] = serde_json::json!("TEST");
        algo["o"]["s"] = serde_json::json!("BTCUSDT");
        algo["o"]["o"] = serde_json::json!("STOP_MARKET");
        algo["o"]["S"] = serde_json::json!("SELL");
        algo["o"]["q"] = serde_json::json!("0.010");
        algo["o"]["tp"] = serde_json::json!("50000.0");
        algo["o"]["p"] = serde_json::json!("0");
        let new_algo: BinanceFuturesAlgoUpdateMsg = serde_json::from_value(algo.clone()).unwrap();
        algo["o"]["X"] = serde_json::json!("FINISHED");
        algo["o"]["ai"] = serde_json::json!("8886774");
        if evicted_actual_id == Some(false) {
            algo["o"].as_object_mut().unwrap().remove("ai");
        }
        algo["o"]["aq"] = serde_json::json!(filled);
        algo["o"]["ap"] = serde_json::json!("49999.0");
        let mut finished: BinanceFuturesAlgoUpdateMsg = serde_json::from_value(algo).unwrap();
        let mut trade: serde_json::Value = serde_json::from_str(&load_fixture_string(
            "futures/user_data_json/order_update_trade.json",
        ))
        .unwrap();

        for field in ["l", "z"] {
            trade["o"][field] = serde_json::json!(filled);
        }
        // Keep the matching-engine execution later than the Algo NEW fixture
        trade["T"] = serde_json::json!(1750515742310_i64);
        trade["E"] = serde_json::json!(1750515742310_i64);
        trade["o"]["T"] = serde_json::json!(1750515742310_i64);
        trade["o"]["q"] = serde_json::json!("0.010");
        trade["o"]["p"] = serde_json::json!("0");
        trade["o"]["sp"] = serde_json::json!("50000.0");
        trade["o"]["ap"] = serde_json::json!("49999.0");
        trade["o"]["L"] = serde_json::json!("49999.0");
        trade["o"]["n"] = serde_json::json!("0.025");
        trade["o"]["m"] = serde_json::json!(false);
        trade["o"]["wt"] = serde_json::json!("MARK_PRICE");
        trade["o"]["S"] = serde_json::json!("SELL");
        trade["o"]["ps"] = serde_json::json!("BOTH");
        trade["o"]["X"] = serde_json::json!(if filled == "0.010" {
            "FILLED"
        } else {
            "PARTIALLY_FILLED"
        });
        trade["o"]["o"] = serde_json::json!("STOP_MARKET");
        trade["o"]["ot"] = serde_json::json!("STOP_MARKET");
        let trade: BinanceFuturesOrderUpdateMsg = serde_json::from_value(trade).unwrap();
        let trades = if split_fills {
            let half = (filled.parse::<Decimal>().unwrap() / Decimal::from(2)).to_string();
            let mut first = trade.clone();
            first.order.last_filled_qty = half.clone();
            first.order.cumulative_filled_qty = half.clone();
            first.order.order_status = crate::common::enums::BinanceOrderStatus::PartiallyFilled;
            first.order.commission = Some("0.0125".to_string());
            let mut second = trade;
            second.order.last_filled_qty = half;
            second.order.trade_id += 1;
            second.order.commission = Some("0.0125".to_string());
            vec![first, second]
        } else {
            vec![trade]
        };
        let state = create_tracked_state_with_price_and_qty(
            client_id,
            instrument_id,
            None,
            Quantity::from("0.010"),
        );
        {
            let mut identity = state.order_identities.get_mut(&client_id).unwrap();
            identity.order_side = OrderSide::Sell;
            identity.order_type = OrderType::StopMarket;
        }
        let triggered = Arc::new(AtomicSet::new());
        let seen = Arc::new(Mutex::new(FifoCache::new()));
        let dispatch_algo = |msg: &BinanceFuturesAlgoUpdateMsg| {
            dispatch_algo_update(
                msg,
                &emitter,
                &http,
                account_id,
                BinanceProductType::UsdM,
                clock,
                &state,
                &triggered,
                false,
            );
        };
        let dispatch_trade = || {
            for trade in &trades {
                let mut lite: BinanceFuturesTradeLiteMsg =
                    load_user_data_fixture("trade_lite.json");
                lite.client_order_id = trade.order.client_order_id.clone();
                lite.symbol = trade.order.symbol;
                lite.order_id = trade.order.order_id;
                lite.trade_id = trade.order.trade_id;
                lite.original_qty = trade.order.original_qty.clone();
                lite.original_price = trade.order.original_price.clone();
                lite.side = trade.order.side;
                lite.last_filled_qty = trade.order.last_filled_qty.clone();
                lite.last_filled_price = trade.order.last_filled_price.clone();
                lite.is_maker = trade.order.is_maker;
                let dispatch_lite = || {
                    dispatch_trade_lite(
                        &lite,
                        &emitter,
                        &http,
                        account_id,
                        BinanceProductType::UsdM,
                        clock,
                        &state,
                        &seen,
                    );
                };

                if trade_lite_first == Some(true) {
                    dispatch_lite();
                    dispatch_lite();
                }
                dispatch_order_update(
                    trade,
                    &emitter,
                    &http,
                    account_id,
                    BinanceProductType::UsdM,
                    clock,
                    &state,
                    true,
                    Decimal::new(4, 4),
                    Currency::USDT(),
                    false,
                    trade_lite_first.is_some(),
                    &seen,
                );

                if trade_lite_first == Some(false) {
                    dispatch_lite();
                    dispatch_lite();
                }
            }
        };

        let cache = Rc::new(RefCell::new(Cache::default()));
        let account = AccountState::new(
            account_id,
            AccountType::Margin,
            vec![AccountBalance::new(
                Money::from("1000 USDT"),
                Money::from("0 USDT"),
                Money::from("1000 USDT"),
            )],
            vec![],
            true,
            UUID4::new(),
            UnixNanos::default(),
            UnixNanos::default(),
            None,
        );
        cache
            .borrow_mut()
            .add_account(AccountAny::Margin(MarginAccount::new(account, true)))
            .unwrap();
        let info: BinanceFuturesUsdExchangeInfo = serde_json::from_str(&load_fixture_string(
            "futures/http_json/exchange_info_usdm.json",
        ))
        .unwrap();
        let symbol = info
            .symbols
            .iter()
            .find(|s| s.symbol.as_str() == "BTCUSDT")
            .unwrap();
        let instrument =
            parse_usdm_instrument(symbol, UnixNanos::default(), UnixNanos::default()).unwrap();
        cache.borrow_mut().add_instrument(instrument).unwrap();
        let order = OrderTestBuilder::new(OrderType::StopMarket)
            .trader_id(TraderId::from("TESTER-001"))
            .strategy_id(StrategyId::from("TEST-STRATEGY"))
            .instrument_id(instrument_id)
            .client_order_id(client_id)
            .side(OrderSide::Sell)
            .quantity(Quantity::from("0.010"))
            .trigger_price(Price::from("50000.0"))
            .build();
        cache
            .borrow_mut()
            .add_order(order.clone(), None, None, true)
            .unwrap();
        let mut engine =
            ExecutionEngine::new(Rc::new(RefCell::new(TestClock::new())), cache.clone(), None);
        engine.register_oms_type(StrategyId::from("TEST-STRATEGY"), OmsType::Netting);
        engine.process(&TestOrderEventStubs::submitted(&order, account_id));
        dispatch_algo(&new_algo);

        if trigger_first {
            finished.algo_order.algo_status = BinanceAlgoStatus::Triggered;
            dispatch_algo(&finished);
            finished.algo_order.algo_status = BinanceAlgoStatus::Finished;
        }

        if cancel_before_trade {
            let mut canceled = trades.last().unwrap().clone();
            canceled.order.execution_type = BinanceExecutionType::Canceled;
            canceled.order.order_status = crate::common::enums::BinanceOrderStatus::Canceled;
            canceled.order.last_filled_qty = "0".to_string();
            dispatch_order_update(
                &canceled,
                &emitter,
                &http,
                account_id,
                BinanceProductType::UsdM,
                clock,
                &state,
                true,
                Decimal::new(4, 4),
                Currency::USDT(),
                false,
                false,
                &seen,
            );
        }

        if finished_first {
            dispatch_algo(&finished);

            if evicted_actual_id.is_some() {
                for index in 0..crate::common::dispatch::FINISHED_ALGO_ORDER_CAPACITY {
                    let cid = ClientOrderId::new(format!("EVICTION-{index}"));
                    state.insert_algo_order_id(cid, VenueOrderId::new(index.to_string()));
                    state.retain_finished_algo_order(cid);
                }
                assert!(!state.order_identities.contains_key(&client_id));
                assert_eq!(cache.borrow().orders(None, None, None, None, None).len(), 1);
            }
            dispatch_trade();
        } else {
            dispatch_trade();
            dispatch_algo(&finished);
        }
        dispatch_trade();
        dispatch_algo(&finished);

        for event in collect_events(&mut rx) {
            match event {
                ExecutionEvent::Order(event) => engine.process(&event),
                ExecutionEvent::Report(report) => engine.reconcile_execution_report(&report),
                other => panic!("Unexpected execution event: {other:?}"),
            }
        }
        let cache = cache.borrow();
        let order = cache.order(&client_id).unwrap();
        assert_eq!(
            order.status(),
            if cancel_before_trade && filled != "0.010" {
                OrderStatus::Canceled
            } else if filled == "0.010" {
                OrderStatus::Filled
            } else {
                OrderStatus::PartiallyFilled
            }
        );
        assert_eq!(order.filled_qty(), Quantity::from(filled));
        assert_eq!(cache.orders(None, None, None, None, None).len(), 1);
        assert_eq!(order.commissions().len(), 1);
        assert_eq!(
            order.commissions().get(&Currency::USDT()),
            Some(&Money::from("0.025 USDT"))
        );
        let fills: Vec<_> = order
            .events()
            .iter()
            .filter_map(|event| match event {
                OrderEventAny::Filled(fill) => Some(fill.trade_id),
                _ => None,
            })
            .collect();
        assert_eq!(
            fills,
            if split_fills {
                vec![TradeId::from("12345678"), TradeId::from("12345679")]
            } else {
                vec![TradeId::from("12345678")]
            }
        );
        let positions = cache.positions(None, Some(&instrument_id), None, None, None);
        assert_eq!(positions.len(), 1);
        assert_eq!(
            positions[0].signed_decimal_qty(),
            -filled.parse::<Decimal>().unwrap()
        );
        assert_eq!(order.venue_order_id(), Some(VenueOrderId::from("8886774")));
        assert_eq!(positions[0].avg_px_open, 49999.0);
        assert_eq!(positions[0].strategy_id, StrategyId::from("TEST-STRATEGY"));
        assert_eq!(positions[0].commissions(), vec![Money::from("0.025 USDT")]);
    }
}
