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

#[cfg(feature = "defi")]
use std::sync::Arc;

#[cfg(feature = "defi")]
use nautilus_model::defi::{Blockchain, Chain, DefiData, Pool, Token};

use super::*;

#[rstest]
fn test_process_instrument(
    #[values(false, true)] typed: bool,
    audusd_sim: CurrencyPair,
    data_engine: Rc<RefCell<DataEngine>>,
    data_client: DataClientAdapter,
) {
    let client_id = data_client.client_id;
    let venue = data_client.venue;
    data_engine.borrow_mut().register_client(data_client, None);

    let audusd_sim = InstrumentAny::CurrencyPair(audusd_sim);

    let sub = SubscribeInstrument::new(
        audusd_sim.id(),
        Some(client_id),
        venue,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );
    let cmd = DataCommand::Subscribe(SubscribeCommand::Instrument(sub));

    data_engine.borrow_mut().execute(cmd);

    let (handler, saving_handler) =
        msgbus::stubs::get_typed_message_saving_handler::<InstrumentAny>(None);
    let topic = switchboard::get_instrument_topic(audusd_sim.id());
    msgbus::subscribe_instruments(topic.into(), handler, None);

    let mut data_engine = data_engine.borrow_mut();
    if typed {
        data_engine.process_data(audusd_sim.clone().into());
    } else {
        data_engine.process(&audusd_sim as &dyn Any);
    }

    let cache = &data_engine.cache().borrow();
    let messages = saving_handler.get_messages();

    assert_eq!(cache.instrument(&audusd_sim.id()).unwrap(), &audusd_sim);
    assert_eq!(messages.len(), 1);
    assert!(messages.contains(&audusd_sim));
}

#[rstest]
#[case::owned(false)]
#[case::borrowed(true)]
fn test_process_instrument_status(
    #[case] borrowed: bool,
    audusd_sim: CurrencyPair,
    data_engine: Rc<RefCell<DataEngine>>,
    data_client: DataClientAdapter,
) {
    let client_id = data_client.client_id;
    let venue = data_client.venue;
    data_engine.borrow_mut().register_client(data_client, None);

    let sub = SubscribeInstrumentStatus::new(
        audusd_sim.id,
        Some(client_id),
        venue,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );
    let cmd = DataCommand::Subscribe(SubscribeCommand::InstrumentStatus(sub));

    data_engine.borrow_mut().execute(cmd);

    let status = InstrumentStatus::new(
        audusd_sim.id,
        MarketStatusAction::Trading,
        UnixNanos::from(1),
        UnixNanos::from(2),
        None,
        None,
        Some(true),
        Some(true),
        None,
    );
    let handler = msgbus::stubs::get_message_saving_handler::<InstrumentStatus>(None);
    let topic = switchboard::get_instrument_status_topic(status.instrument_id);
    msgbus::subscribe_any(topic.into(), handler.clone(), None);

    let mut data_engine = data_engine.borrow_mut();
    dispatch_data(&mut data_engine, Data::InstrumentStatus(status), borrowed);
    let cache = data_engine.cache().borrow();
    let messages = msgbus::stubs::get_saved_messages::<InstrumentStatus>(&handler);

    assert_eq!(messages.len(), 1);
    assert!(messages.contains(&status));
    assert_eq!(cache.instrument_status(&audusd_sim.id), Some(&status));
    assert_eq!(
        cache.instrument_statuses(&audusd_sim.id),
        Some(vec![status]),
    );
}

#[rstest]
#[case::owned(false)]
#[case::borrowed(true)]
fn test_process_instrument_close(
    #[case] borrowed: bool,
    audusd_sim: CurrencyPair,
    data_engine: Rc<RefCell<DataEngine>>,
) {
    let close = InstrumentClose::new(
        audusd_sim.id,
        Price::from("0.8000"),
        InstrumentCloseType::EndOfSession,
        UnixNanos::from(1),
        UnixNanos::from(2),
    );
    let (handler, saver) = get_any_saving_handler::<InstrumentClose>(None);
    let topic = switchboard::get_instrument_close_topic(close.instrument_id);
    msgbus::subscribe_any(topic.into(), handler, None);

    dispatch_data(
        &mut data_engine.borrow_mut(),
        Data::InstrumentClose(close),
        borrowed,
    );

    assert_eq!(saver.get_messages(), vec![close]);
}

#[rstest]
fn test_process_instrument_status_through_any(
    audusd_sim: CurrencyPair,
    data_engine: Rc<RefCell<DataEngine>>,
    data_client: DataClientAdapter,
) {
    let client_id = data_client.client_id;
    let venue = data_client.venue;
    data_engine.borrow_mut().register_client(data_client, None);

    let sub = SubscribeInstrumentStatus::new(
        audusd_sim.id,
        Some(client_id),
        venue,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );
    let cmd = DataCommand::Subscribe(SubscribeCommand::InstrumentStatus(sub));
    data_engine.borrow_mut().execute(cmd);

    let status = InstrumentStatus::new(
        audusd_sim.id,
        MarketStatusAction::Trading,
        UnixNanos::from(1),
        UnixNanos::from(2),
        None,
        None,
        Some(true),
        Some(true),
        None,
    );
    let handler = msgbus::stubs::get_message_saving_handler::<InstrumentStatus>(None);
    let topic = switchboard::get_instrument_status_topic(status.instrument_id);
    msgbus::subscribe_any(topic.into(), handler.clone(), None);

    let mut data_engine = data_engine.borrow_mut();
    // Drive through the process() entrypoint with `&dyn Any`
    data_engine.process(&status as &dyn Any);
    let cache = data_engine.cache().borrow();
    let messages = msgbus::stubs::get_saved_messages::<InstrumentStatus>(&handler);

    assert_eq!(messages.len(), 1);
    assert!(messages.contains(&status));
    assert_eq!(cache.instrument_status(&audusd_sim.id), Some(&status));
}

#[rstest]
fn test_process_instrument_status_updates_existing(
    audusd_sim: CurrencyPair,
    data_engine: Rc<RefCell<DataEngine>>,
    data_client: DataClientAdapter,
) {
    let client_id = data_client.client_id;
    let venue = data_client.venue;
    data_engine.borrow_mut().register_client(data_client, None);

    let sub = SubscribeInstrumentStatus::new(
        audusd_sim.id,
        Some(client_id),
        venue,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );
    let cmd = DataCommand::Subscribe(SubscribeCommand::InstrumentStatus(sub));
    data_engine.borrow_mut().execute(cmd);

    let status1 = InstrumentStatus::new(
        audusd_sim.id,
        MarketStatusAction::PreOpen,
        UnixNanos::from(1),
        UnixNanos::from(2),
        None,
        None,
        Some(false),
        Some(false),
        None,
    );

    let status2 = InstrumentStatus::new(
        audusd_sim.id,
        MarketStatusAction::Trading,
        UnixNanos::from(3),
        UnixNanos::from(4),
        None,
        None,
        Some(true),
        Some(true),
        None,
    );

    let mut data_engine = data_engine.borrow_mut();
    data_engine.process_data(Data::InstrumentStatus(status1));
    data_engine.process_data(Data::InstrumentStatus(status2));
    let cache = data_engine.cache().borrow();

    assert_eq!(cache.instrument_status(&audusd_sim.id), Some(&status2));
    assert_eq!(
        cache.instrument_statuses(&audusd_sim.id),
        Some(vec![status2, status1]),
    );
}

#[rstest]
fn test_trim_to_bounds_trims_instruments(audusd_sim: CurrencyPair, venue: Venue) {
    let mut earlier = audusd_sim.clone();
    earlier.ts_init = UnixNanos::from(1_000);
    let mut middle = audusd_sim.clone();
    middle.ts_init = UnixNanos::from(2_000);
    let mut later = audusd_sim;
    later.ts_init = UnixNanos::from(3_000);

    let mut resp = DataResponse::Instruments(InstrumentsResponse::new(
        UUID4::new(),
        ClientId::test_default(),
        venue,
        vec![
            InstrumentAny::CurrencyPair(earlier),
            InstrumentAny::CurrencyPair(middle),
            InstrumentAny::CurrencyPair(later),
        ],
        Some(UnixNanos::from(2_000)),
        Some(UnixNanos::from(2_000)),
        UnixNanos::default(),
        None,
    ));

    resp.trim_to_bounds();

    let DataResponse::Instruments(instruments) = resp else {
        panic!("expected Instruments variant");
    };

    let ts_inits: Vec<u64> = instruments
        .data
        .iter()
        .map(|i| Instrument::ts_init(i).as_u64())
        .collect();
    assert_eq!(ts_inits, vec![2_000]);
}

#[cfg(feature = "defi")]
#[rstest]
#[case(false)]
#[case(true)]
fn test_token_instrument_publication_requires_valid_cache_insert(
    #[case] conflicting: bool,
    data_engine: Rc<RefCell<DataEngine>>,
    audusd_sim: CurrencyPair,
) {
    let token = Token::new(
        Arc::new(Chain::new(
            Blockchain::Ethereum,
            if conflicting { 812_402 } else { 812_403 },
        )),
        "0x3141592653589793238462643383279502884197"
            .parse()
            .unwrap(),
        "Publication token".into(),
        if conflicting { "PUBBAD" } else { "PUBGOOD" }.into(),
        6,
    );
    let mut pair = audusd_sim;
    pair.id = InstrumentId::from("PUBLICATION.DEFI");
    pair.base_currency = token.currency().unwrap();
    if conflicting {
        pair.base_currency.precision = 18;
    }

    let instrument = pair.into_any();
    let (handler, saver) = msgbus::stubs::get_typed_message_saving_handler::<InstrumentAny>(None);
    let topic = switchboard::get_instrument_topic(instrument.id());
    msgbus::subscribe_instruments(topic.into(), handler, None);

    data_engine
        .borrow_mut()
        .process_data(instrument.clone().into());

    let messages = saver.get_messages();
    let engine = data_engine.borrow();
    let cache = engine.cache().borrow();

    if conflicting {
        assert_eq!(messages.len(), 0);
        assert_eq!(cache.instrument(&instrument.id()), None);
    } else {
        assert_eq!(messages.len(), 1);
        assert_eq!(
            serde_json::to_value(&messages[0]).unwrap(),
            serde_json::to_value(&instrument).unwrap()
        );
        assert_eq!(messages[0].base_currency().unwrap().precision, 6);
        assert_eq!(
            serde_json::to_value(cache.instrument(&instrument.id()).unwrap()).unwrap(),
            serde_json::to_value(&instrument).unwrap()
        );
        assert_eq!(
            cache
                .instrument(&instrument.id())
                .unwrap()
                .base_currency()
                .unwrap()
                .precision,
            6
        );
    }
}

#[cfg(feature = "defi")]
#[rstest]
fn test_conflicting_pool_retains_instrument_and_pool(data_engine: Rc<RefCell<DataEngine>>) {
    let (mut pool, _) = crate::common::defi::make_initialized_pool_and_swap();
    let chain = Arc::new(Chain::new(Blockchain::GaladrielDevnet, 823_130));
    pool.chain = Arc::clone(&chain);
    pool.token0.chain = Arc::clone(&chain);
    pool.token1.chain = chain;
    let original = pool.clone();
    let (instrument_handler, instrument_saver) =
        msgbus::stubs::get_typed_message_saving_handler::<InstrumentAny>(None);
    let (pool_handler, pool_saver) = msgbus::stubs::get_typed_message_saving_handler::<Pool>(None);
    let instrument_topic = switchboard::get_instrument_topic(pool.instrument_id);
    let pool_topic = nautilus_common::defi::switchboard::get_defi_pool_topic(pool.instrument_id);
    msgbus::subscribe_instruments(instrument_topic.into(), instrument_handler, None);
    msgbus::subscribe_defi_pools(pool_topic.into(), pool_handler, None);
    data_engine
        .borrow_mut()
        .process_defi_data(DefiData::Pool(pool.clone()));
    pool.token1.decimals = 17;

    data_engine
        .borrow_mut()
        .process_defi_data(DefiData::Pool(pool));

    let engine = data_engine.borrow();
    let cache = engine.cache().borrow();
    assert_eq!(pool_saver.get_messages(), vec![original.clone()]);
    assert_eq!(instrument_saver.get_messages().len(), 1);
    assert_eq!(cache.pool(&original.instrument_id), Some(&original));
    assert_eq!(
        cache.pool(&original.instrument_id).unwrap().token1.decimals,
        6
    );
    assert_eq!(
        cache
            .instrument(&original.instrument_id)
            .unwrap()
            .quote_currency()
            .precision,
        6
    );
}

#[cfg(feature = "defi")]
#[rstest]
fn test_stale_legacy_instrument_is_not_published(
    data_engine: Rc<RefCell<DataEngine>>,
    audusd_sim: CurrencyPair,
) {
    let token = Token::new(
        Arc::new(Chain::new(Blockchain::Ethereum, 823_131)),
        "0x3141592653589793238462643383279502884197"
            .parse()
            .unwrap(),
        "Publication token".into(),
        "PUBSTALE".into(),
        6,
    );
    let mut pair = audusd_sim.clone();
    pair.base_currency = token.currency().unwrap();
    let canonical = pair.into_any();
    let (handler, saver) = msgbus::stubs::get_typed_message_saving_handler::<InstrumentAny>(None);
    let topic = switchboard::get_instrument_topic(canonical.id());
    msgbus::subscribe_instruments(topic.into(), handler, None);
    data_engine
        .borrow()
        .cache()
        .borrow_mut()
        .add_instrument(canonical.clone())
        .unwrap();

    data_engine
        .borrow_mut()
        .process_data(audusd_sim.into_any().into());

    let engine = data_engine.borrow();
    let cache = engine.cache().borrow();
    assert_eq!(saver.get_messages().len(), 0);
    assert_eq!(
        serde_json::to_value(cache.instrument(&canonical.id()).unwrap()).unwrap(),
        serde_json::to_value(canonical).unwrap()
    );
}

#[cfg(feature = "defi")]
#[rstest]
#[case(false, false)]
#[case(false, true)]
#[case(true, false)]
#[case(true, true)]
fn test_instrument_response_rejects_currency_conflicts(
    #[case] bulk: bool,
    #[case] conflicting: bool,
    data_engine: Rc<RefCell<DataEngine>>,
    audusd_sim: CurrencyPair,
) {
    let case = u32::from(bulk) * 2 + u32::from(conflicting);

    let token = Token::new(
        Arc::new(Chain::new(Blockchain::Ethereum, 823_150 + case)),
        "0x3141592653589793238462643383279502884197"
            .parse()
            .unwrap(),
        "Response token".into(),
        format!("REPLY{case}"),
        6,
    );
    let mut canonical = audusd_sim;
    canonical.base_currency = token.currency().unwrap();
    let expected = canonical.clone().into_any();
    let mut incoming = canonical.clone();
    if conflicting {
        std::mem::swap(&mut incoming.base_currency, &mut incoming.quote_currency);
    }

    data_engine
        .borrow()
        .cache()
        .borrow_mut()
        .add_instrument(expected.clone())
        .unwrap();
    let root = UUID4::new();
    let child = UUID4::new();
    let (handler, saver) =
        msgbus::stubs::get_typed_message_saving_handler::<DataResponseEnvelope>(None);
    msgbus::register_response_handler(&root, handler);

    let mut later = canonical.clone();
    later.id = InstrumentId::from("LATER.SIM");
    let expected_later = later.clone().into_any();

    let response = if bulk {
        let mut sibling = canonical;
        sibling.id = InstrumentId::from("SIBLING.SIM");
        DataResponse::Instruments(InstrumentsResponse::new(
            child,
            ClientId::test_default(),
            sibling.id.venue,
            vec![sibling.into_any(), incoming.into_any(), later.into_any()],
            None,
            None,
            UnixNanos::from(123),
            None,
        ))
    } else {
        DataResponse::Instrument(Box::new(InstrumentResponse::new(
            child,
            ClientId::test_default(),
            incoming.id,
            incoming.into_any(),
            None,
            None,
            UnixNanos::from(123),
            None,
        )))
    };

    let envelope = DataResponseEnvelope::new(response, true, Some(child)).with_correlation_id(root);
    data_engine.borrow_mut().response_envelope(envelope.clone());

    let expected_envelope = if conflicting {
        DataResponseEnvelope::terminal(
            root,
            Some("Instrument currency integrity check failed".into()),
        )
    } else {
        envelope
    };

    let replies = saver.get_messages();
    let engine = data_engine.borrow();
    let cache = engine.cache().borrow();
    assert_eq!(replies.len(), 1);
    assert_eq!(
        serde_json::to_value(&replies[0]).unwrap(),
        serde_json::to_value(expected_envelope).unwrap()
    );
    assert_eq!(
        serde_json::to_value(cache.instrument(&expected.id()).unwrap()).unwrap(),
        serde_json::to_value(expected).unwrap()
    );

    if bulk {
        assert_eq!(
            serde_json::to_value(cache.instrument(&expected_later.id()).unwrap()).unwrap(),
            serde_json::to_value(expected_later).unwrap()
        );
    }
}
