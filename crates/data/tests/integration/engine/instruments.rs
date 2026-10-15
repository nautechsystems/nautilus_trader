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
fn test_process_instrument(
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
    data_engine.process(&audusd_sim as &dyn Any);
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
