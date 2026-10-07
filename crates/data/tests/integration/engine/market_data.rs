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

use super::*;

#[rstest]
#[case::owned(false)]
#[case::borrowed(true)]
fn test_process_quote_tick(
    #[case] borrowed: bool,
    audusd_sim: CurrencyPair,
    data_engine: Rc<RefCell<DataEngine>>,
    data_client: DataClientAdapter,
) {
    let client_id = data_client.client_id;
    let venue = data_client.venue;
    data_engine.borrow_mut().register_client(data_client, None);

    let sub = SubscribeQuotes::new(
        audusd_sim.id,
        Some(client_id),
        venue,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );
    let cmd = DataCommand::Subscribe(SubscribeCommand::Quotes(sub));

    data_engine.borrow_mut().execute(cmd);

    let quote = QuoteTick::default();
    let (handler, saver) = get_typed_message_saving_handler::<QuoteTick>(None);
    let topic = switchboard::get_quotes_topic(quote.instrument_id);
    msgbus::subscribe_quotes(topic.into(), handler, None);

    let mut data_engine = data_engine.borrow_mut();
    dispatch_data(&mut data_engine, Data::Quote(quote), borrowed);
    let cache = &data_engine.cache().borrow();
    let messages = saver.get_messages();

    assert_eq!(cache.quote(&quote.instrument_id), Some(quote).as_ref());
    assert_eq!(messages.len(), 1);
    assert!(messages.contains(&quote));
}

#[rstest]
#[case::owned(false)]
#[case::borrowed(true)]
fn test_process_trade_tick(
    #[case] borrowed: bool,
    audusd_sim: CurrencyPair,
    data_engine: Rc<RefCell<DataEngine>>,
    data_client: DataClientAdapter,
) {
    let client_id = data_client.client_id;
    let venue = data_client.venue;
    data_engine.borrow_mut().register_client(data_client, None);

    let sub = SubscribeTrades::new(
        audusd_sim.id,
        Some(client_id),
        venue,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );
    let cmd = DataCommand::Subscribe(SubscribeCommand::Trades(sub));

    data_engine.borrow_mut().execute(cmd);

    let trade = TradeTick::default();
    let (handler, saver) = get_typed_message_saving_handler::<TradeTick>(None);
    let topic = switchboard::get_trades_topic(trade.instrument_id);
    msgbus::subscribe_trades(topic.into(), handler, None);

    let mut data_engine = data_engine.borrow_mut();
    dispatch_data(&mut data_engine, Data::Trade(trade), borrowed);
    let cache = &data_engine.cache().borrow();
    let messages = saver.get_messages();

    assert_eq!(cache.trade(&trade.instrument_id), Some(trade).as_ref());
    assert_eq!(messages.len(), 1);
    assert!(messages.contains(&trade));
}

#[rstest]
fn test_synthetic_quote_and_trade_commands_do_not_forward_to_client(
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
    venue: Venue,
) {
    let _ = stub_msgbus;
    let clock = Rc::new(RefCell::new(VirtualClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    let engine_clock: Rc<RefCell<dyn Clock>> = clock.clone();
    let mut data_engine = DataEngine::new(engine_clock, cache.clone(), None);
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        clock,
        cache.clone(),
        client_id,
        venue,
        None,
        &recorder,
        &mut data_engine,
    );

    let (synthetic, _, _) = synthetic_index();
    let synthetic_id = synthetic.id;
    cache.borrow_mut().add_synthetic(synthetic).unwrap();

    data_engine.execute(DataCommand::Subscribe(SubscribeCommand::Quotes(
        SubscribeQuotes::new(
            synthetic_id,
            Some(client_id),
            Some(venue),
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        ),
    )));
    data_engine.execute(DataCommand::Subscribe(SubscribeCommand::Trades(
        SubscribeTrades::new(
            synthetic_id,
            Some(client_id),
            Some(venue),
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        ),
    )));
    let subscribed_quotes = data_engine.subscribed_synthetic_quotes();
    let subscribed_trades = data_engine.subscribed_synthetic_trades();

    data_engine.execute(DataCommand::Unsubscribe(UnsubscribeCommand::Quotes(
        UnsubscribeQuotes::new(
            synthetic_id,
            Some(client_id),
            Some(venue),
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        ),
    )));
    data_engine.execute(DataCommand::Unsubscribe(UnsubscribeCommand::Trades(
        UnsubscribeTrades::new(
            synthetic_id,
            Some(client_id),
            Some(venue),
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        ),
    )));

    assert!(subscribed_quotes.contains(&synthetic_id));
    assert!(subscribed_trades.contains(&synthetic_id));
    assert!(
        !data_engine
            .subscribed_synthetic_quotes()
            .contains(&synthetic_id)
    );
    assert!(
        !data_engine
            .subscribed_synthetic_trades()
            .contains(&synthetic_id)
    );
    assert!(recorder.borrow().is_empty());
}

#[rstest]
fn test_trim_to_bounds_trims_trades(audusd_sim: CurrencyPair) {
    let instrument_id = audusd_sim.id;

    let make_trade = |ts: u64, trade_id: &str| {
        TradeTick::new(
            instrument_id,
            Price::from("1.00000"),
            Quantity::from("1"),
            AggressorSide::Buy,
            TradeId::new(trade_id),
            UnixNanos::from(ts),
            UnixNanos::from(ts),
        )
    };

    let mut resp = DataResponse::Trades(TradesResponse::new(
        UUID4::new(),
        ClientId::test_default(),
        instrument_id,
        vec![
            make_trade(1_000, "t1"),
            make_trade(2_000, "t2"),
            make_trade(3_000, "t3"),
        ],
        Some(UnixNanos::from(2_000)),
        Some(UnixNanos::from(2_000)),
        UnixNanos::default(),
        None,
    ));

    resp.trim_to_bounds();

    let DataResponse::Trades(trades) = resp else {
        panic!("expected Trades variant");
    };

    let ts_inits: Vec<u64> = trades.data.iter().map(|t| t.ts_init.as_u64()).collect();
    assert_eq!(ts_inits, vec![2_000]);
}
