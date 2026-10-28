#![expect(
    clippy::redundant_clone,
    reason = "test cases clone commands to assert ownership and recorder state"
)]

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

use ahash::AHashSet;
use nautilus_common::{
    cache::Cache,
    clock::VirtualClock,
    messages::{
        SubscribeCommand, UnsubscribeCommand,
        data::{
            DataCommand,
            // Request commands
            RequestBars,
            RequestBookDepth,
            RequestBookSnapshot,
            RequestCommand,
            RequestCustomData,
            RequestFundingRates,
            RequestInstrument,
            RequestInstruments,
            RequestQuotes,
            RequestTrades,
            // Subscription commands
            SubscribeBars,
            SubscribeBookDeltas,
            SubscribeBookDepth,
            SubscribeCustomData,
            SubscribeFundingRates,
            SubscribeIndexPrices,
            SubscribeInstrument,
            SubscribeInstrumentClose,
            SubscribeInstrumentStatus,
            SubscribeInstruments,
            SubscribeMarkPrices,
            SubscribeQuotes,
            SubscribeTrades,
            UnsubscribeBars,
            UnsubscribeBookDeltas,
            UnsubscribeBookDepth,
            UnsubscribeCustomData,
            UnsubscribeFundingRates,
            UnsubscribeIndexPrices,
            UnsubscribeInstrument,
            UnsubscribeInstrumentClose,
            UnsubscribeInstrumentStatus,
            UnsubscribeInstruments,
            UnsubscribeMarkPrices,
            UnsubscribeQuotes,
            UnsubscribeTrades,
        },
    },
};
use nautilus_core::{Params, UUID4, UnixNanos};
use nautilus_data::client::DataClientAdapter;
use nautilus_model::{
    data::{BarType, DataType},
    enums::BookType,
    identifiers::{ClientId, InstrumentId, Venue},
    instruments::stubs::audusd_sim,
    stubs::TestDefault,
};
use rstest::{fixture, rstest};
#[cfg(feature = "defi")]
use {
    nautilus_common::messages::defi::{
        DefiSubscribeCommand, DefiUnsubscribeCommand, SubscribeBlocks, SubscribePool,
        SubscribePoolSwaps, UnsubscribeBlocks, UnsubscribePool, UnsubscribePoolSwaps,
    },
    nautilus_model::defi::Blockchain,
};

use crate::common::mocks::{MockDataClient, MockSubscribeFailure};

#[fixture]
fn clock() -> Rc<RefCell<VirtualClock>> {
    Rc::new(RefCell::new(VirtualClock::new()))
}

#[fixture]
fn cache() -> Rc<RefCell<Cache>> {
    Rc::new(RefCell::new(Cache::default()))
}

#[fixture]
fn client_id() -> ClientId {
    ClientId::new("TEST-CLIENT")
}

#[fixture]
fn venue() -> Venue {
    Venue::test_default()
}

// --------------------------------------------------------------------------------------------
// Subscription handler tests
// --------------------------------------------------------------------------------------------

#[rstest]
fn test_custom_data_subscription(
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let client = Box::new(MockDataClient::new(clock, cache, client_id, Some(venue)));
    let mut adapter = DataClientAdapter::new(
        client_id,
        Some(venue),
        false, // handles deltas
        false, // handles snapshots
        client,
    );

    // Define a custom data type
    let data_type = DataType::new("MyType", None, None);

    let sub = SubscribeCommand::Data(SubscribeCustomData::new(
        Some(client_id),
        Some(venue),
        data_type.clone(),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    ));
    adapter.execute_subscribe(sub.clone());
    assert!(adapter.subscriptions_custom.contains(&data_type));

    // Idempotency check
    adapter.execute_subscribe(sub.clone());
    assert_eq!(adapter.subscriptions_custom.len(), 1);

    let unsub = UnsubscribeCommand::Data(UnsubscribeCustomData::new(
        Some(client_id),
        Some(venue),
        data_type.clone(),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    ));
    adapter.execute_unsubscribe(&unsub);

    assert!(!adapter.subscriptions_custom.contains(&data_type));
}

#[rstest]
fn test_custom_data_subscription_retries_after_client_failure(
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let recorder = Rc::new(RefCell::new(Vec::new()));
    let client = Box::new(
        MockDataClient::new_with_recorder(
            clock,
            cache,
            client_id,
            Some(venue),
            Some(Rc::clone(&recorder)),
        )
        .with_custom_subscribe_failure(),
    );
    let mut adapter = DataClientAdapter::new(client_id, Some(venue), false, false, client);
    let data_type = DataType::new("RetryType", None, None);
    let subscribe = |command_id| {
        SubscribeCommand::Data(SubscribeCustomData::new(
            Some(client_id),
            Some(venue),
            data_type.clone(),
            command_id,
            UnixNanos::default(),
            None,
            None,
        ))
    };

    adapter.execute_subscribe(subscribe(UUID4::new()));
    assert!(!adapter.subscriptions_custom.contains(&data_type));
    assert!(recorder.borrow().is_empty());

    adapter.execute_subscribe(subscribe(UUID4::new()));
    assert!(adapter.subscriptions_custom.contains(&data_type));
    assert_eq!(recorder.borrow().len(), 1);
    recorder.borrow_mut().clear();

    adapter.execute_unsubscribe(&UnsubscribeCommand::Data(UnsubscribeCustomData::new(
        Some(client_id),
        Some(venue),
        data_type.clone(),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    )));

    assert!(!adapter.subscriptions_custom.contains(&data_type));
    assert_eq!(recorder.borrow().len(), 1);
}

#[rstest]
#[case::retry(false)]
#[case::reacquire(true)]
fn test_custom_data_unsubscription_retries_after_client_failure(
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
    #[case] reacquire: bool,
) {
    let recorder = Rc::new(RefCell::new(Vec::new()));
    let client = Box::new(
        MockDataClient::new_with_recorder(
            clock,
            cache,
            client_id,
            Some(venue),
            Some(Rc::clone(&recorder)),
        )
        .with_custom_unsubscribe_failure(),
    );
    let mut adapter = DataClientAdapter::new(client_id, Some(venue), false, false, client);
    let data_type = DataType::new("RetryType", None, None);
    let params = Params::new([("route".to_string(), serde_json::json!(37))].into());
    adapter.execute_subscribe(SubscribeCommand::Data(SubscribeCustomData::new(
        Some(client_id),
        Some(venue),
        data_type.clone(),
        UUID4::new(),
        UnixNanos::default(),
        None,
        Some(params.clone()),
    )));
    recorder.borrow_mut().clear();

    let unsubscribe = |command_id, ts_init| {
        UnsubscribeCommand::Data(UnsubscribeCustomData::new(
            Some(client_id),
            Some(venue),
            data_type.clone(),
            command_id,
            ts_init,
            None,
            None,
        ))
    };
    adapter.execute_unsubscribe(&unsubscribe(UUID4::new(), UnixNanos::from(1)));

    assert!(adapter.subscriptions_custom.contains(&data_type));
    assert!(recorder.borrow().is_empty());

    if reacquire {
        adapter.execute_subscribe(SubscribeCommand::Data(SubscribeCustomData::new(
            Some(client_id),
            Some(venue),
            data_type.clone(),
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        )));
    }
    assert!(adapter.subscriptions_custom.contains(&data_type));
    assert!(recorder.borrow().is_empty());

    let retry = unsubscribe(UUID4::new(), UnixNanos::from(2));
    adapter.execute_unsubscribe(&retry);

    assert!(!adapter.subscriptions_custom.contains(&data_type));
    let UnsubscribeCommand::Data(mut expected) = retry else {
        unreachable!()
    };
    expected.params = Some(params);
    let recorded = recorder.borrow();
    let [DataCommand::Unsubscribe(UnsubscribeCommand::Data(actual))] = recorded.as_slice() else {
        panic!("expected one successful custom data unsubscribe");
    };
    assert_eq!(
        serde_json::to_value(actual).unwrap(),
        serde_json::to_value(expected).unwrap()
    );
}

#[rstest]
fn test_instrument_subscription(
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let client = Box::new(MockDataClient::new(clock, cache, client_id, Some(venue)));
    let mut adapter = DataClientAdapter::new(client_id, Some(venue), false, false, client);

    let instrument = audusd_sim();
    let inst_id = instrument.id;

    let sub = SubscribeCommand::Instrument(SubscribeInstrument::new(
        inst_id,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    ));
    adapter.execute_subscribe(sub.clone());
    assert!(adapter.subscriptions_instrument.contains(&inst_id));

    // Idempotency check
    adapter.execute_subscribe(sub.clone());
    assert_eq!(adapter.subscriptions_instrument.len(), 1);

    let unsub = UnsubscribeCommand::Instrument(UnsubscribeInstrument::new(
        inst_id,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    ));
    adapter.execute_unsubscribe(&unsub);
    assert!(!adapter.subscriptions_instrument.contains(&inst_id));
}

#[rstest]
fn test_instruments_subscription(
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let client = Box::new(MockDataClient::new(clock, cache, client_id, Some(venue)));
    let mut adapter = DataClientAdapter::new(client_id, Some(venue), false, false, client);

    let sub = SubscribeCommand::Instruments(SubscribeInstruments::new(
        Some(client_id),
        venue,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    ));
    adapter.execute_subscribe(sub.clone());
    assert!(adapter.subscriptions_instrument_venue.contains(&venue));

    // Idempotency check
    adapter.execute_subscribe(sub.clone());
    assert_eq!(adapter.subscriptions_instrument_venue.len(), 1);

    let unsub = UnsubscribeCommand::Instruments(UnsubscribeInstruments::new(
        Some(client_id),
        venue,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    ));
    adapter.execute_unsubscribe(&unsub);
    assert!(!adapter.subscriptions_instrument_venue.contains(&venue));
}

#[rstest]
fn test_book_deltas_subscription(
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let client = Box::new(MockDataClient::new(clock, cache, client_id, Some(venue)));
    let mut adapter = DataClientAdapter::new(client_id, Some(venue), false, false, client);

    let instrument = audusd_sim();
    let inst_id = instrument.id;
    let depth = NonZeroUsize::new(1);

    let sub = SubscribeCommand::BookDeltas(SubscribeBookDeltas::new(
        inst_id,
        BookType::L2_MBP,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        depth,
        false,
        None,
        None,
    ));
    adapter.execute_subscribe(sub.clone());
    assert!(adapter.subscriptions_book_deltas.contains(&inst_id));

    // Idempotency check
    adapter.execute_subscribe(sub.clone());
    assert_eq!(adapter.subscriptions_book_deltas.len(), 1);

    let unsub = UnsubscribeCommand::BookDeltas(UnsubscribeBookDeltas::new(
        inst_id,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    ));
    adapter.execute_unsubscribe(&unsub);
    assert!(!adapter.subscriptions_book_deltas.contains(&inst_id));
}

#[rstest]
fn test_book_depth_subscription(
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let client = Box::new(MockDataClient::new(clock, cache, client_id, Some(venue)));
    let mut adapter = DataClientAdapter::new(client_id, Some(venue), false, false, client);

    let instrument = audusd_sim();
    let inst_id = instrument.id;
    let depth = NonZeroUsize::new(10);

    let sub = SubscribeCommand::BookDepth(SubscribeBookDepth::new(
        inst_id,
        BookType::L2_MBP,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        depth,
        false,
        None,
        None,
    ));
    adapter.execute_subscribe(sub.clone());
    assert!(adapter.subscriptions_book_depth.contains(&inst_id));

    // Idempotency check
    adapter.execute_subscribe(sub.clone());
    assert_eq!(adapter.subscriptions_book_depth.len(), 1);

    let unsub = UnsubscribeCommand::BookDepth(UnsubscribeBookDepth::new(
        inst_id,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    ));
    adapter.execute_unsubscribe(&unsub);
    assert!(!adapter.subscriptions_book_depth.contains(&inst_id));
}

#[rstest]
fn test_quote_subscription(
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let client = Box::new(MockDataClient::new(clock, cache, client_id, Some(venue)));
    let mut adapter = DataClientAdapter::new(client_id, Some(venue), false, false, client);

    let instrument = audusd_sim();
    let inst_id = instrument.id;

    let sub = SubscribeCommand::Quotes(SubscribeQuotes::new(
        inst_id,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    ));
    adapter.execute_subscribe(sub.clone());
    assert!(adapter.subscriptions_quotes.contains(&inst_id));

    // Idempotency check
    adapter.execute_subscribe(sub.clone());
    assert_eq!(adapter.subscriptions_quotes.len(), 1);

    let unsub = UnsubscribeCommand::Quotes(UnsubscribeQuotes::new(
        inst_id,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    ));
    adapter.execute_unsubscribe(&unsub);
    assert!(!adapter.subscriptions_quotes.contains(&inst_id));
}

#[rstest]
fn test_trades_subscription(
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let client = Box::new(MockDataClient::new(clock, cache, client_id, Some(venue)));
    let mut adapter = DataClientAdapter::new(client_id, Some(venue), false, false, client);

    let instrument = audusd_sim();
    let inst_id = instrument.id;

    let sub = SubscribeCommand::Trades(SubscribeTrades::new(
        inst_id,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    ));
    adapter.execute_subscribe(sub.clone());
    assert!(adapter.subscriptions_trades.contains(&inst_id));

    // Idempotency check
    adapter.execute_subscribe(sub.clone());
    assert_eq!(adapter.subscriptions_trades.len(), 1);

    let unsub = UnsubscribeCommand::Trades(UnsubscribeTrades::new(
        inst_id,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    ));
    adapter.execute_unsubscribe(&unsub);
    assert!(!adapter.subscriptions_trades.contains(&inst_id));
}

#[rstest]
fn test_mark_price_subscription(
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let client = Box::new(MockDataClient::new(clock, cache, client_id, Some(venue)));
    let mut adapter = DataClientAdapter::new(client_id, Some(venue), false, false, client);

    let instrument = audusd_sim();
    let inst_id = instrument.id;

    let sub = SubscribeCommand::MarkPrices(SubscribeMarkPrices::new(
        inst_id,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    ));
    adapter.execute_subscribe(sub.clone());
    assert!(adapter.subscriptions_mark_prices.contains(&inst_id));

    // Idempotency check
    adapter.execute_subscribe(sub.clone());
    assert_eq!(adapter.subscriptions_mark_prices.len(), 1);

    let unsub = UnsubscribeCommand::MarkPrices(UnsubscribeMarkPrices::new(
        inst_id,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    ));
    adapter.execute_unsubscribe(&unsub);
    assert!(!adapter.subscriptions_mark_prices.contains(&inst_id));
}

#[rstest]
fn test_index_price_subscription(
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let client = Box::new(MockDataClient::new(clock, cache, client_id, Some(venue)));
    let mut adapter = DataClientAdapter::new(client_id, Some(venue), false, false, client);

    let instrument = audusd_sim();
    let inst_id = instrument.id;

    let sub = SubscribeCommand::IndexPrices(SubscribeIndexPrices::new(
        inst_id,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    ));
    adapter.execute_subscribe(sub.clone());
    assert!(adapter.subscriptions_index_prices.contains(&inst_id));

    // Idempotency check
    adapter.execute_subscribe(sub.clone());
    assert_eq!(adapter.subscriptions_index_prices.len(), 1);

    let unsub = UnsubscribeCommand::IndexPrices(UnsubscribeIndexPrices::new(
        inst_id,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    ));
    adapter.execute_unsubscribe(&unsub);
    assert!(!adapter.subscriptions_index_prices.contains(&inst_id));
}

#[rstest]
fn test_funding_rate_subscription(
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let client = Box::new(MockDataClient::new(clock, cache, client_id, Some(venue)));
    let mut adapter = DataClientAdapter::new(client_id, Some(venue), false, false, client);

    let instrument = audusd_sim();
    let inst_id = instrument.id;

    let sub = SubscribeCommand::FundingRates(SubscribeFundingRates::new(
        inst_id,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    ));
    adapter.execute_subscribe(sub.clone());
    assert!(adapter.subscriptions_funding_rates.contains(&inst_id));

    // Idempotency check
    adapter.execute_subscribe(sub.clone());
    assert_eq!(adapter.subscriptions_funding_rates.len(), 1);

    let unsub = UnsubscribeCommand::FundingRates(UnsubscribeFundingRates::new(
        inst_id,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    ));
    adapter.execute_unsubscribe(&unsub);
    assert!(!adapter.subscriptions_funding_rates.contains(&inst_id));
}

#[rstest]
fn test_bars_subscription(
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let client = Box::new(MockDataClient::new(clock, cache, client_id, Some(venue)));
    let mut adapter = DataClientAdapter::new(client_id, Some(venue), false, false, client);

    let bar_type: BarType = "AUDUSD.SIM-1-MINUTE-LAST-INTERNAL".into();

    let sub = SubscribeCommand::Bars(SubscribeBars::new(
        bar_type,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    ));
    adapter.execute_subscribe(sub.clone());
    assert!(adapter.subscriptions_bars.contains(&bar_type));

    // Idempotency check
    adapter.execute_subscribe(sub.clone());
    assert_eq!(adapter.subscriptions_bars.len(), 1);

    let unsub = UnsubscribeCommand::Bars(UnsubscribeBars::new(
        bar_type,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    ));
    adapter.execute_unsubscribe(&unsub);
    assert!(!adapter.subscriptions_bars.contains(&bar_type));
}

#[rstest]
fn test_instrument_status_subscription(
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let client = Box::new(MockDataClient::new(clock, cache, client_id, Some(venue)));
    let mut adapter = DataClientAdapter::new(client_id, Some(venue), false, false, client);

    let instrument = audusd_sim();
    let inst_id = instrument.id;

    let sub = SubscribeCommand::InstrumentStatus(SubscribeInstrumentStatus::new(
        inst_id,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    ));
    adapter.execute_subscribe(sub.clone());
    assert!(adapter.subscriptions_instrument_status.contains(&inst_id));

    // Idempotency check
    adapter.execute_subscribe(sub.clone());
    assert_eq!(adapter.subscriptions_instrument_status.len(), 1);

    let unsub = UnsubscribeCommand::InstrumentStatus(UnsubscribeInstrumentStatus::new(
        inst_id,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    ));
    adapter.execute_unsubscribe(&unsub);
    assert!(!adapter.subscriptions_instrument_status.contains(&inst_id));
}

#[rstest]
fn test_instrument_close_subscription(
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let client = Box::new(MockDataClient::new(clock, cache, client_id, Some(venue)));
    let mut adapter = DataClientAdapter::new(client_id, Some(venue), false, false, client);

    let instrument = audusd_sim();
    let inst_id = instrument.id;

    let sub = SubscribeCommand::InstrumentClose(SubscribeInstrumentClose::new(
        inst_id,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    ));
    adapter.execute_subscribe(sub.clone());
    assert!(adapter.subscriptions_instrument_close.contains(&inst_id));

    // Idempotency check
    adapter.execute_subscribe(sub.clone());
    assert_eq!(adapter.subscriptions_instrument_close.len(), 1);

    let unsub = UnsubscribeCommand::InstrumentClose(UnsubscribeInstrumentClose::new(
        inst_id,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    ));
    adapter.execute_unsubscribe(&unsub);
    assert!(!adapter.subscriptions_instrument_close.contains(&inst_id));
}

#[rstest]
fn test_custom_data_unsubscribe_noop(
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let client = Box::new(MockDataClient::new(clock, cache, client_id, Some(venue)));
    let mut adapter = DataClientAdapter::new(client_id, Some(venue), false, false, client);

    // Unsubscribe without prior subscribe should be no-op
    let data_type = DataType::new("NoOpType", None, None);
    let unsub = UnsubscribeCommand::Data(UnsubscribeCustomData::new(
        Some(client_id),
        Some(venue),
        data_type.clone(),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    ));
    adapter.execute_unsubscribe(&unsub);
    assert!(!adapter.subscriptions_custom.contains(&data_type));
    // Underlying client should not have been called (state-only test)
    assert!(adapter.subscriptions_custom.is_empty());
}

#[rstest]
fn test_custom_data_unsubscribe_idempotent(
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let client = Box::new(MockDataClient::new(clock, cache, client_id, Some(venue)));
    let mut adapter = DataClientAdapter::new(client_id, Some(venue), false, false, client);

    // Subscribe then unsubscribe twice
    let data_type = DataType::new("IdemType", None, None);
    let sub = SubscribeCommand::Data(SubscribeCustomData::new(
        Some(client_id),
        Some(venue),
        data_type.clone(),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    ));
    adapter.execute_subscribe(sub.clone());
    let unsub = UnsubscribeCommand::Data(UnsubscribeCustomData::new(
        Some(client_id),
        Some(venue),
        data_type.clone(),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    ));
    adapter.execute_unsubscribe(&unsub);
    adapter.execute_unsubscribe(&unsub);
    // Expect adapter state cleared and no panic on second unsubscribe
    assert!(!adapter.subscriptions_custom.contains(&data_type));
}

#[rstest]
fn test_custom_data_unsubscribe_keeps_client_subscription_when_subscribers_remain(
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let recorder = Rc::new(RefCell::new(Vec::new()));
    let client = Box::new(MockDataClient::new_with_recorder(
        clock,
        cache,
        client_id,
        Some(venue),
        Some(Rc::clone(&recorder)),
    ));
    let mut adapter = DataClientAdapter::new(client_id, Some(venue), false, false, client);
    let data_type = DataType::new("SharedType", None, None);
    let first_params = Params::new([("owner".to_string(), serde_json::json!(1))].into());
    let second_params = Params::new([("owner".to_string(), serde_json::json!(2))].into());
    let first_subscribe = SubscribeCommand::Data(SubscribeCustomData::new(
        Some(client_id),
        Some(venue),
        data_type.clone(),
        UUID4::new(),
        UnixNanos::default(),
        None,
        Some(first_params.clone()),
    ));
    let second_subscribe = SubscribeCommand::Data(SubscribeCustomData::new(
        Some(client_id),
        Some(venue),
        data_type.clone(),
        UUID4::new(),
        UnixNanos::default(),
        None,
        Some(second_params),
    ));
    adapter.execute_subscribe(first_subscribe);
    adapter.execute_subscribe(second_subscribe);
    assert_eq!(recorder.borrow().len(), 1);
    recorder.borrow_mut().clear();

    let first_unsubscribe = UnsubscribeCommand::Data(UnsubscribeCustomData::new(
        Some(client_id),
        Some(venue),
        data_type.clone(),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    ));
    adapter.execute_unsubscribe(&first_unsubscribe);

    assert!(adapter.subscriptions_custom.contains(&data_type));
    assert!(recorder.borrow().is_empty());

    let second_unsubscribe = UnsubscribeCommand::Data(UnsubscribeCustomData::new(
        Some(client_id),
        Some(venue),
        data_type.clone(),
        UUID4::new(),
        UnixNanos::from(2),
        None,
        None,
    ));
    adapter.execute_unsubscribe(&second_unsubscribe);
    let recorded = recorder.borrow();

    assert!(!adapter.subscriptions_custom.contains(&data_type));
    let [DataCommand::Unsubscribe(UnsubscribeCommand::Data(command))] = recorded.as_slice() else {
        panic!("expected one retained custom-data unsubscribe, was {recorded:?}");
    };
    assert_eq!(command.data_type, data_type);
    assert_eq!(command.client_id, Some(client_id));
    assert_eq!(command.venue, Some(venue));
    assert_eq!(command.ts_init, UnixNanos::from(2));
    assert_eq!(command.params.as_ref(), Some(&first_params));
}

#[rstest]
fn test_instrument_unsubscribe_noop(
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let client = Box::new(MockDataClient::new(clock, cache, client_id, Some(venue)));
    let mut adapter = DataClientAdapter::new(client_id, Some(venue), false, false, client);

    // Unsubscribe instrument without prior subscribe
    let inst_id = audusd_sim().id;
    let unsub = UnsubscribeCommand::Instrument(UnsubscribeInstrument::new(
        inst_id,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    ));
    adapter.execute_unsubscribe(&unsub);
    assert!(!adapter.subscriptions_instrument.contains(&inst_id));
    // Underlying client should not have been called
    assert!(adapter.subscriptions_instrument.is_empty());
}

#[rstest]
fn test_instrument_unsubscribe_idempotent(
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let client = Box::new(MockDataClient::new(clock, cache, client_id, Some(venue)));
    let mut adapter = DataClientAdapter::new(client_id, Some(venue), false, false, client);
    // Subscribe then unsubscribe twice
    let inst_id = audusd_sim().id;
    let sub = SubscribeCommand::Instrument(SubscribeInstrument::new(
        inst_id,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    ));
    adapter.execute_subscribe(sub.clone());
    let unsub = UnsubscribeCommand::Instrument(UnsubscribeInstrument::new(
        inst_id,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    ));
    adapter.execute_unsubscribe(&unsub);
    adapter.execute_unsubscribe(&unsub);
    // Expect adapter state cleared and no panic on second unsubscribe
    assert!(!adapter.subscriptions_instrument.contains(&inst_id));
}

#[rstest]
fn test_instruments_unsubscribe_noop(
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let client = Box::new(MockDataClient::new(clock, cache, client_id, Some(venue)));
    let mut adapter = DataClientAdapter::new(client_id, Some(venue), false, false, client);

    // Unsubscribe instruments without prior subscribe
    let unsub = UnsubscribeCommand::Instruments(UnsubscribeInstruments::new(
        Some(client_id),
        venue,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    ));
    adapter.execute_unsubscribe(&unsub);
    assert!(adapter.subscriptions_instrument_venue.is_empty());
}

#[rstest]
fn test_instruments_unsubscribe_idempotent(
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let client = Box::new(MockDataClient::new(clock, cache, client_id, Some(venue)));
    let mut adapter = DataClientAdapter::new(client_id, Some(venue), false, false, client);

    // Subscribe then unsubscribe twice
    let sub = SubscribeCommand::Instruments(SubscribeInstruments::new(
        Some(client_id),
        venue,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    ));
    adapter.execute_subscribe(sub.clone());

    let unsub = UnsubscribeCommand::Instruments(UnsubscribeInstruments::new(
        Some(client_id),
        venue,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    ));

    adapter.execute_unsubscribe(&unsub);
    adapter.execute_unsubscribe(&unsub);
    assert!(adapter.subscriptions_instrument_venue.is_empty());
}
#[rstest]
fn test_book_deltas_unsubscribe_noop(
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let client = Box::new(MockDataClient::new(clock, cache, client_id, Some(venue)));
    let mut adapter = DataClientAdapter::new(client_id, Some(venue), false, false, client);
    // Unsubscribe book deltas without subscribe
    let inst_id = audusd_sim().id;

    let unsub = UnsubscribeCommand::BookDeltas(UnsubscribeBookDeltas::new(
        inst_id,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    ));
    adapter.execute_unsubscribe(&unsub);
    assert!(adapter.subscriptions_book_deltas.is_empty());
}

#[rstest]
fn test_book_deltas_unsubscribe_idempotent(
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let client = Box::new(MockDataClient::new(clock, cache, client_id, Some(venue)));
    let mut adapter = DataClientAdapter::new(client_id, Some(venue), false, false, client);
    let inst_id = audusd_sim().id;

    let sub = SubscribeCommand::BookDeltas(SubscribeBookDeltas::new(
        inst_id,
        BookType::L2_MBP,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        NonZeroUsize::new(1),
        false,
        None,
        None,
    ));
    adapter.execute_subscribe(sub.clone());

    let unsub = UnsubscribeCommand::BookDeltas(UnsubscribeBookDeltas::new(
        inst_id,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    ));

    adapter.execute_unsubscribe(&unsub);
    adapter.execute_unsubscribe(&unsub);
    assert!(adapter.subscriptions_book_deltas.is_empty());
}

#[rstest]
fn test_book_depth_unsubscribe_noop(
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let client = Box::new(MockDataClient::new(clock, cache, client_id, Some(venue)));
    let mut adapter = DataClientAdapter::new(client_id, Some(venue), false, false, client);
    let inst_id = audusd_sim().id;
    let unsub = UnsubscribeCommand::BookDepth(UnsubscribeBookDepth::new(
        inst_id,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    ));
    adapter.execute_unsubscribe(&unsub);
    assert!(adapter.subscriptions_book_depth.is_empty());
}

#[rstest]
fn test_book_depth_unsubscribe_idempotent(
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let client = Box::new(MockDataClient::new(clock, cache, client_id, Some(venue)));
    let mut adapter = DataClientAdapter::new(client_id, Some(venue), false, false, client);
    let inst_id = audusd_sim().id;
    let sub = SubscribeCommand::BookDepth(SubscribeBookDepth::new(
        inst_id,
        BookType::L2_MBP,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        NonZeroUsize::new(10),
        false,
        None,
        None,
    ));
    adapter.execute_subscribe(sub.clone());
    let unsub = UnsubscribeCommand::BookDepth(UnsubscribeBookDepth::new(
        inst_id,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    ));
    adapter.execute_unsubscribe(&unsub);
    adapter.execute_unsubscribe(&unsub);
    assert!(adapter.subscriptions_book_depth.is_empty());
}

macro_rules! subscription_batch_tests {
    ($release_test:ident, $failed_test:ident, $partial_test:ident, $retry_test:ident, $subscribe:ident, $unsubscribe:ident, $partial_failure:ident, $variant:ident, $subscriptions:ident, $unsubscribe_failure:ident) => {
        #[rstest]
        #[case::independent(false)]
        #[case::overlapping(true)]
        fn $release_test(
            #[case] overlapping: bool,
            clock: Rc<RefCell<VirtualClock>>,
            cache: Rc<RefCell<Cache>>,
            client_id: ClientId,
            venue: Venue,
        ) {
            let recorder = Rc::new(RefCell::new(Vec::new()));
            let client = MockDataClient::new_with_recorder(
                clock,
                cache,
                client_id,
                Some(venue),
                Some(Rc::clone(&recorder)),
            );
            let mut adapter =
                DataClientAdapter::new(client_id, Some(venue), false, false, Box::new(client));
            let first = audusd_sim().id;
            let second = InstrumentId::from("ETH/USD.SIM");
            let params: Params = serde_json::from_value(serde_json::json!({"route": 37})).unwrap();

            let first_subscribe = $subscribe::new(
                first,
                Some(client_id),
                Some(venue),
                UUID4::new(),
                UnixNanos::from(11),
                Some(UUID4::new()),
                Some(params.clone()),
            );

            let second_subscribe = $subscribe::new(
                if overlapping {
                    vec![first, second]
                } else {
                    vec![second]
                },
                Some(client_id),
                Some(venue),
                UUID4::new(),
                UnixNanos::from(13),
                Some(UUID4::new()),
                Some(params.clone()),
            );

            adapter.execute_subscribe(SubscribeCommand::$variant(
                first_subscribe.clone(),
            ));
            adapter.execute_subscribe(SubscribeCommand::$variant(
                second_subscribe.clone(),
            ));

            if overlapping {
                let release = SubscribeCommand::$variant(first_subscribe.clone())
                    .into_unsubscribe(UUID4::new(), UnixNanos::from(17), Some(UUID4::new()));
                adapter.execute_unsubscribe(&release);
                assert_eq!(recorder.borrow().len(), 2);
                assert_eq!(
                    adapter.$subscriptions,
                    AHashSet::from_iter([first, second])
                );
            }

            let release = $unsubscribe::new(
                [first, second],
                Some(ClientId::from("FALLBACK")),
                Some(Venue::from("OTHER")),
                UUID4::new(),
                UnixNanos::from(19),
                Some(UUID4::new()),
                None,
            );
            adapter.execute_unsubscribe(&UnsubscribeCommand::$variant(
                release.clone(),
            ));
            let second_dispatch = second_subscribe.with_instrument_ids(second);

            let first_release = $unsubscribe::new(
                first,
                Some(client_id),
                Some(venue),
                release.command_id,
                release.ts_init,
                release.correlation_id,
                Some(params.clone()),
            );
            let second_release = first_release.with_instrument_ids(second);
            let recorded = recorder.borrow();

            let [
                DataCommand::Subscribe(SubscribeCommand::$variant(
                    actual_first_subscribe,
                )),
                DataCommand::Subscribe(SubscribeCommand::$variant(
                    actual_second_subscribe,
                )),
                DataCommand::Unsubscribe(UnsubscribeCommand::$variant(
                    actual_first_release,
                )),
                DataCommand::Unsubscribe(UnsubscribeCommand::$variant(
                    actual_second_release,
                )),
            ] = recorded.as_slice()
            else {
                panic!("expected separate physical acquisitions and releases, was {recorded:?}");
            };

            assert_eq!(
                serde_json::to_value([actual_first_subscribe, actual_second_subscribe]).unwrap(),
                serde_json::to_value([first_subscribe, second_dispatch]).unwrap(),
            );
            assert_eq!(
                serde_json::to_value([actual_first_release, actual_second_release]).unwrap(),
                serde_json::to_value([first_release, second_release]).unwrap(),
            );
            assert_eq!(adapter.$subscriptions, AHashSet::new());
        }

        #[rstest]
        #[case::final_first_retry_first(false, true)]
        #[case::shared_first_retry_first(true, true)]
        #[case::final_first_other_owner_first(false, false)]
        #[case::shared_first_other_owner_first(true, false)]
        fn $failed_test(
            #[case] shared_first: bool,
            #[case] retry_first: bool,
            clock: Rc<RefCell<VirtualClock>>,
            cache: Rc<RefCell<Cache>>,
            client_id: ClientId,
            venue: Venue,
        ) {
            let recorder = Rc::new(RefCell::new(Vec::new()));
            let client = MockDataClient::new_with_recorder(
                clock,
                cache,
                client_id,
                Some(venue),
                Some(Rc::clone(&recorder)),
            )
            .$unsubscribe_failure();
            let mut adapter =
                DataClientAdapter::new(client_id, Some(venue), false, false, Box::new(client));
            let first = audusd_sim().id;
            let shared = InstrumentId::from("ETH/USD.SIM");
            let first_params: Params =
                serde_json::from_value(serde_json::json!({"route": 37})).unwrap();
            let shared_params: Params =
                serde_json::from_value(serde_json::json!({"route": 59})).unwrap();

            let first_command = $subscribe::new(
                first,
                Some(client_id),
                Some(venue),
                UUID4::new(),
                UnixNanos::from(11),
                None,
                Some(first_params.clone()),
            );

            let shared_command = $subscribe::new(
                shared,
                Some(client_id),
                Some(venue),
                UUID4::new(),
                UnixNanos::from(13),
                None,
                Some(shared_params.clone()),
            );
            adapter.execute_subscribe(SubscribeCommand::$variant(first_command));
            adapter.execute_subscribe(SubscribeCommand::$variant(
                shared_command.clone(),
            ));
            adapter.execute_subscribe(SubscribeCommand::$variant(
                $subscribe {
                    command_id: UUID4::new(),
                    ..shared_command
                },
            ));

            let ids = if shared_first {
                [shared, first]
            } else {
                [first, shared]
            };

            let release = $unsubscribe::new(
                ids,
                Some(client_id),
                Some(venue),
                UUID4::new(),
                UnixNanos::from(17),
                Some(UUID4::new()),
                None,
            );
            adapter.execute_unsubscribe(&UnsubscribeCommand::$variant(
                release.clone(),
            ));
            assert_eq!(
                adapter.$subscriptions,
                AHashSet::from_iter([first, shared])
            );
            let retry = release.with_instrument_ids(vec![first]);

            if retry_first {
                adapter.execute_unsubscribe(&UnsubscribeCommand::$variant(
                    retry.clone(),
                ));
                assert_eq!(
                    adapter.$subscriptions,
                    AHashSet::from_iter([shared])
                );
            }

            let final_release = $unsubscribe::new(
                shared,
                Some(client_id),
                Some(venue),
                UUID4::new(),
                UnixNanos::from(19),
                Some(UUID4::new()),
                None,
            );
            adapter.execute_unsubscribe(&UnsubscribeCommand::$variant(
                final_release.clone(),
            ));

            if !retry_first {
                assert_eq!(
                    adapter.$subscriptions,
                    AHashSet::from_iter([first])
                );
                adapter.execute_unsubscribe(&UnsubscribeCommand::$variant(
                    retry.clone(),
                ));
            }

            let commands = recorder.borrow();

            let [
                DataCommand::Subscribe(_),
                DataCommand::Subscribe(_),
                DataCommand::Unsubscribe(UnsubscribeCommand::$variant(
                    actual_release_one,
                )),
                DataCommand::Unsubscribe(UnsubscribeCommand::$variant(
                    actual_release_two,
                )),
            ] = commands.as_slice()
            else {
                panic!("expected one physical release per ID, was {commands:?}");
            };

            let expected_first = $unsubscribe::new(
                first,
                Some(client_id),
                Some(venue),
                release.command_id,
                release.ts_init,
                release.correlation_id,
                Some(first_params),
            );

            let expected_shared = $unsubscribe::new(
                shared,
                Some(client_id),
                Some(venue),
                final_release.command_id,
                final_release.ts_init,
                final_release.correlation_id,
                Some(shared_params),
            );

            let expected = if retry_first {
                [expected_first, expected_shared]
            } else {
                [expected_shared, expected_first]
            };

            assert_eq!(
                serde_json::to_value([actual_release_one, actual_release_two]).unwrap(),
                serde_json::to_value(expected).unwrap()
            );
            assert_eq!(adapter.$subscriptions, AHashSet::new());
        }

        #[rstest]
        fn $partial_test(
            clock: Rc<RefCell<VirtualClock>>,
            cache: Rc<RefCell<Cache>>,
            client_id: ClientId,
            venue: Venue,
        ) {
            let first = audusd_sim().id;
            let second = InstrumentId::from("ETH/USD.SIM");
            let recorder = Rc::new(RefCell::new(Vec::new()));
            let client = MockDataClient::new_with_recorder(
                clock,
                cache,
                client_id,
                Some(venue),
                Some(Rc::clone(&recorder)),
            )
            .with_subscribe_failure(MockSubscribeFailure::$partial_failure(first));
            let mut adapter =
                DataClientAdapter::new(client_id, Some(venue), false, false, Box::new(client));
            let original_params: Params =
                serde_json::from_value(serde_json::json!({"route": 37})).unwrap();

            let subscribe = $subscribe::new(
                [first, second],
                Some(client_id),
                Some(venue),
                UUID4::new(),
                UnixNanos::from(23),
                Some(UUID4::new()),
                Some(original_params.clone()),
            );
            adapter.execute_subscribe(SubscribeCommand::$variant(subscribe.clone()));
            assert_eq!(
                adapter.$subscriptions,
                AHashSet::from_iter([first])
            );
            adapter.execute_subscribe(SubscribeCommand::$variant(subscribe.clone()));
            assert_eq!(
                adapter.$subscriptions,
                AHashSet::from_iter([first, second])
            );
            let fallback_params: Params =
                serde_json::from_value(serde_json::json!({"route": 59})).unwrap();

            let release = $unsubscribe::new(
                [second, first],
                Some(ClientId::from("FALLBACK")),
                Some(Venue::from("OTHER")),
                UUID4::new(),
                UnixNanos::from(31),
                Some(UUID4::new()),
                Some(fallback_params),
            );
            adapter.execute_unsubscribe(&UnsubscribeCommand::$variant(
                release.clone(),
            ));
            let expected_subscribes = [
                subscribe.with_instrument_ids(first),
                subscribe.with_instrument_ids(second),
            ];

            let expected_release = $unsubscribe::new(
                [second, first],
                Some(client_id),
                Some(venue),
                release.command_id,
                release.ts_init,
                release.correlation_id,
                Some(original_params),
            );
            let commands = recorder.borrow();

            let [
                DataCommand::Subscribe(SubscribeCommand::$variant(first_subscribe)),
                DataCommand::Subscribe(SubscribeCommand::$variant(second_subscribe)),
                DataCommand::Unsubscribe(UnsubscribeCommand::$variant(
                    actual_release,
                )),
            ] = commands.as_slice()
            else {
                panic!(
                    "expected each instrument ID subscribed once and one final release, was {commands:?}"
                );
            };

            assert_eq!(
                serde_json::to_value([first_subscribe, second_subscribe]).unwrap(),
                serde_json::to_value(expected_subscribes).unwrap()
            );
            assert_eq!(
                serde_json::to_value(actual_release).unwrap(),
                serde_json::to_value(expected_release).unwrap()
            );
            assert_eq!(adapter.$subscriptions, AHashSet::new());
        }

        #[rstest]
        #[case::single(false)]
        #[case::batch(true)]
        fn $retry_test(
            #[case] batch: bool,
            clock: Rc<RefCell<VirtualClock>>,
            cache: Rc<RefCell<Cache>>,
            client_id: ClientId,
            venue: Venue,
        ) {
            let recorder = Rc::new(RefCell::new(Vec::new()));
            let client = MockDataClient::new_with_recorder(
                clock,
                cache,
                client_id,
                Some(venue),
                Some(Rc::clone(&recorder)),
            )
            .$unsubscribe_failure();
            let mut adapter =
                DataClientAdapter::new(client_id, Some(venue), false, false, Box::new(client));
            let first = audusd_sim().id;
            let second = InstrumentId::from("ETH/USD.SIM");

            let ids = if batch {
                vec![second, first]
            } else {
                vec![first]
            };

            let release_ids: Vec<_> = ids.iter().rev().copied().collect();
            let original_params: Params =
                serde_json::from_value(serde_json::json!({"route": 37})).unwrap();
            let fallback_params: Params =
                serde_json::from_value(serde_json::json!({"route": 59})).unwrap();

            let subscribe = $subscribe::new(
                &ids,
                Some(client_id),
                Some(venue),
                UUID4::new(),
                UnixNanos::from(23),
                Some(UUID4::new()),
                Some(original_params.clone()),
            );
            adapter.execute_subscribe(SubscribeCommand::$variant(subscribe.clone()));

            let failed_release = $unsubscribe::new(
                &release_ids,
                Some(ClientId::from("FALLBACK")),
                Some(Venue::from("OTHER")),
                UUID4::new(),
                UnixNanos::from(31),
                Some(UUID4::new()),
                Some(fallback_params),
            );
            adapter.execute_unsubscribe(&UnsubscribeCommand::$variant(
                failed_release.clone(),
            ));
            assert_eq!(
                adapter.$subscriptions,
                ids.iter().copied().collect::<AHashSet<_>>()
            );
            assert_eq!(recorder.borrow().len(), 1);

            let retry = $unsubscribe {
                command_id: UUID4::new(),
                ts_init: UnixNanos::from(47),
                correlation_id: Some(UUID4::new()),
                ..failed_release
            };

            adapter
                .execute_unsubscribe(&UnsubscribeCommand::$variant(retry.clone()));

            let expected = $unsubscribe::new(
                release_ids,
                Some(client_id),
                Some(venue),
                retry.command_id,
                retry.ts_init,
                retry.correlation_id,
                Some(original_params),
            );
            let commands = recorder.borrow();

            let [
                DataCommand::Subscribe(SubscribeCommand::$variant(actual_subscribe)),
                DataCommand::Unsubscribe(UnsubscribeCommand::$variant(
                    actual_release,
                )),
            ] = commands.as_slice()
            else {
                panic!("expected one subscribe and one successful quote release, was {commands:?}");
            };

            assert_eq!(
                serde_json::to_value(actual_subscribe).unwrap(),
                serde_json::to_value(subscribe).unwrap()
            );
            assert_eq!(
                serde_json::to_value(actual_release).unwrap(),
                serde_json::to_value(expected).unwrap()
            );
            assert_eq!(adapter.$subscriptions, AHashSet::new());
        }
    };
}

subscription_batch_tests!(
    test_quote_release_keeps_distinct_acquisitions_separate,
    test_quote_batch_failed_release_preserves_other_members,
    test_quote_batch_partial_failure_tracks_accepted_ids_and_retries_missing,
    test_quotes_unsubscribe_failure_retries_original_route,
    SubscribeQuotes,
    UnsubscribeQuotes,
    QuotesPartial,
    Quotes,
    subscriptions_quotes,
    with_quotes_unsubscribe_failure
);

subscription_batch_tests!(
    test_trade_release_keeps_distinct_acquisitions_separate,
    test_trade_batch_failed_release_preserves_other_members,
    test_trade_batch_partial_failure_tracks_accepted_ids_and_retries_missing,
    test_trades_unsubscribe_failure_retries_original_route,
    SubscribeTrades,
    UnsubscribeTrades,
    TradesPartial,
    Trades,
    subscriptions_trades,
    with_trades_unsubscribe_failure
);

#[rstest]
fn test_quotes_unsubscribe_noop(
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let client = Box::new(MockDataClient::new(clock, cache, client_id, Some(venue)));
    let mut adapter = DataClientAdapter::new(client_id, Some(venue), false, false, client);
    let inst_id = audusd_sim().id;
    let unsub = UnsubscribeCommand::Quotes(UnsubscribeQuotes::new(
        inst_id,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    ));
    adapter.execute_unsubscribe(&unsub);
    assert!(adapter.subscriptions_quotes.is_empty());
}

#[rstest]
fn test_quotes_unsubscribe_idempotent(
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let client = Box::new(MockDataClient::new(clock, cache, client_id, Some(venue)));
    let mut adapter = DataClientAdapter::new(client_id, Some(venue), false, false, client);
    let inst_id = audusd_sim().id;
    let sub = SubscribeCommand::Quotes(SubscribeQuotes::new(
        inst_id,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    ));
    adapter.execute_subscribe(sub.clone());
    let unsub = UnsubscribeCommand::Quotes(UnsubscribeQuotes::new(
        inst_id,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    ));
    adapter.execute_unsubscribe(&unsub);
    adapter.execute_unsubscribe(&unsub);
    assert!(adapter.subscriptions_quotes.is_empty());
}

#[rstest]
fn test_trades_unsubscribe_noop(
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let client = Box::new(MockDataClient::new(clock, cache, client_id, Some(venue)));
    let mut adapter = DataClientAdapter::new(client_id, Some(venue), false, false, client);
    let inst_id = audusd_sim().id;
    let unsub = UnsubscribeCommand::Trades(UnsubscribeTrades::new(
        inst_id,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    ));
    adapter.execute_unsubscribe(&unsub);
    assert!(adapter.subscriptions_trades.is_empty());
}

#[rstest]
fn test_trades_unsubscribe_idempotent(
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let client = Box::new(MockDataClient::new(clock, cache, client_id, Some(venue)));
    let mut adapter = DataClientAdapter::new(client_id, Some(venue), false, false, client);
    let inst_id = audusd_sim().id;
    let sub = SubscribeCommand::Trades(SubscribeTrades::new(
        inst_id,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    ));
    adapter.execute_subscribe(sub.clone());
    let unsub = UnsubscribeCommand::Trades(UnsubscribeTrades::new(
        inst_id,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    ));
    adapter.execute_unsubscribe(&unsub);
    adapter.execute_unsubscribe(&unsub);
    assert!(adapter.subscriptions_trades.is_empty());
}

#[rstest]
fn test_bars_unsubscribe_noop(
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let client = Box::new(MockDataClient::new(clock, cache, client_id, Some(venue)));
    let mut adapter = DataClientAdapter::new(client_id, Some(venue), false, false, client);
    let bar_type: BarType = "AUDUSD.SIM-1-MINUTE-LAST-INTERNAL".into();
    let unsub = UnsubscribeCommand::Bars(UnsubscribeBars::new(
        bar_type,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    ));
    adapter.execute_unsubscribe(&unsub);
    assert!(adapter.subscriptions_bars.is_empty());
}

#[rstest]
fn test_bars_unsubscribe_idempotent(
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let client = Box::new(MockDataClient::new(clock, cache, client_id, Some(venue)));
    let mut adapter = DataClientAdapter::new(client_id, Some(venue), false, false, client);
    let bar_type: BarType = "AUDUSD.SIM-1-MINUTE-LAST-INTERNAL".into();
    let sub = SubscribeCommand::Bars(SubscribeBars::new(
        bar_type,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    ));
    adapter.execute_subscribe(sub.clone());
    let unsub = UnsubscribeCommand::Bars(UnsubscribeBars::new(
        bar_type,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    ));
    adapter.execute_unsubscribe(&unsub);
    adapter.execute_unsubscribe(&unsub);
    assert!(adapter.subscriptions_bars.is_empty());
}

#[rstest]
fn test_mark_prices_unsubscribe_noop(
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let client = Box::new(MockDataClient::new(clock, cache, client_id, Some(venue)));
    let mut adapter = DataClientAdapter::new(client_id, Some(venue), false, false, client);
    let inst_id = audusd_sim().id;
    let unsub = UnsubscribeCommand::MarkPrices(UnsubscribeMarkPrices::new(
        inst_id,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    ));
    adapter.execute_unsubscribe(&unsub);
    assert!(adapter.subscriptions_mark_prices.is_empty());
}

#[rstest]
fn test_mark_prices_unsubscribe_idempotent(
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let client = Box::new(MockDataClient::new(clock, cache, client_id, Some(venue)));
    let mut adapter = DataClientAdapter::new(client_id, Some(venue), false, false, client);
    let inst_id = audusd_sim().id;
    let sub = SubscribeCommand::MarkPrices(SubscribeMarkPrices::new(
        inst_id,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    ));
    adapter.execute_subscribe(sub.clone());
    let unsub = UnsubscribeCommand::MarkPrices(UnsubscribeMarkPrices::new(
        inst_id,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    ));
    adapter.execute_unsubscribe(&unsub);
    adapter.execute_unsubscribe(&unsub);
    assert!(adapter.subscriptions_mark_prices.is_empty());
}

#[rstest]
fn test_index_prices_unsubscribe_noop(
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let client = Box::new(MockDataClient::new(clock, cache, client_id, Some(venue)));
    let mut adapter = DataClientAdapter::new(client_id, Some(venue), false, false, client);
    let inst_id = audusd_sim().id;
    let unsub = UnsubscribeCommand::IndexPrices(UnsubscribeIndexPrices::new(
        inst_id,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    ));
    adapter.execute_unsubscribe(&unsub);
    assert!(adapter.subscriptions_index_prices.is_empty());
}

#[rstest]
fn test_index_prices_unsubscribe_idempotent(
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let client = Box::new(MockDataClient::new(clock, cache, client_id, Some(venue)));
    let mut adapter = DataClientAdapter::new(client_id, Some(venue), false, false, client);
    let inst_id = audusd_sim().id;
    let sub = SubscribeCommand::IndexPrices(SubscribeIndexPrices::new(
        inst_id,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    ));
    adapter.execute_subscribe(sub.clone());
    let unsub = UnsubscribeCommand::IndexPrices(UnsubscribeIndexPrices::new(
        inst_id,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    ));
    adapter.execute_unsubscribe(&unsub);
    adapter.execute_unsubscribe(&unsub);
    assert!(adapter.subscriptions_index_prices.is_empty());
}

#[rstest]
fn test_funding_rates_unsubscribe_noop(
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let client = Box::new(MockDataClient::new(clock, cache, client_id, Some(venue)));
    let mut adapter = DataClientAdapter::new(client_id, Some(venue), false, false, client);

    let inst_id = audusd_sim().id;
    let unsub = UnsubscribeCommand::FundingRates(UnsubscribeFundingRates::new(
        inst_id,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    ));
    adapter.execute_unsubscribe(&unsub);
    assert!(!adapter.subscriptions_funding_rates.contains(&inst_id));
}

#[rstest]
fn test_funding_rates_unsubscribe_idempotent(
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let client = Box::new(MockDataClient::new(clock, cache, client_id, Some(venue)));
    let mut adapter = DataClientAdapter::new(client_id, Some(venue), false, false, client);

    let inst_id = audusd_sim().id;
    let sub = SubscribeCommand::FundingRates(SubscribeFundingRates::new(
        inst_id,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    ));
    adapter.execute_subscribe(sub.clone());
    assert!(adapter.subscriptions_funding_rates.contains(&inst_id));

    let unsub = UnsubscribeCommand::FundingRates(UnsubscribeFundingRates::new(
        inst_id,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    ));
    adapter.execute_unsubscribe(&unsub);
    assert!(!adapter.subscriptions_funding_rates.contains(&inst_id));

    adapter.execute_unsubscribe(&unsub);
    assert!(!adapter.subscriptions_funding_rates.contains(&inst_id));
}

#[rstest]
fn test_instrument_status_unsubscribe_noop(
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let client = Box::new(MockDataClient::new(clock, cache, client_id, Some(venue)));
    let mut adapter = DataClientAdapter::new(client_id, Some(venue), false, false, client);
    let inst_id = audusd_sim().id;
    let unsub = UnsubscribeCommand::InstrumentStatus(UnsubscribeInstrumentStatus::new(
        inst_id,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    ));
    adapter.execute_unsubscribe(&unsub);
    assert!(adapter.subscriptions_instrument_status.is_empty());
}

#[rstest]
fn test_instrument_status_unsubscribe_idempotent(
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let client = Box::new(MockDataClient::new(clock, cache, client_id, Some(venue)));
    let mut adapter = DataClientAdapter::new(client_id, Some(venue), false, false, client);
    let inst_id = audusd_sim().id;
    let sub = SubscribeCommand::InstrumentStatus(SubscribeInstrumentStatus::new(
        inst_id,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    ));
    adapter.execute_subscribe(sub.clone());
    let unsub = UnsubscribeCommand::InstrumentStatus(UnsubscribeInstrumentStatus::new(
        inst_id,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    ));
    adapter.execute_unsubscribe(&unsub);
    adapter.execute_unsubscribe(&unsub);
    assert!(adapter.subscriptions_instrument_status.is_empty());
}

#[rstest]
fn test_instrument_close_unsubscribe_noop(
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let client = Box::new(MockDataClient::new(clock, cache, client_id, Some(venue)));
    let mut adapter = DataClientAdapter::new(client_id, Some(venue), false, false, client);
    let inst_id = audusd_sim().id;
    let unsub = UnsubscribeCommand::InstrumentClose(UnsubscribeInstrumentClose::new(
        inst_id,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    ));
    adapter.execute_unsubscribe(&unsub);
    assert!(adapter.subscriptions_instrument_close.is_empty());
}

#[rstest]
fn test_instrument_close_unsubscribe_idempotent(
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let client = Box::new(MockDataClient::new(clock, cache, client_id, Some(venue)));
    let mut adapter = DataClientAdapter::new(client_id, Some(venue), false, false, client);
    let inst_id = audusd_sim().id;

    let sub = SubscribeCommand::InstrumentClose(SubscribeInstrumentClose::new(
        inst_id,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    ));
    adapter.execute_subscribe(sub.clone());

    let unsub = UnsubscribeCommand::InstrumentClose(UnsubscribeInstrumentClose::new(
        inst_id,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    ));
    adapter.execute_unsubscribe(&unsub);
    adapter.execute_unsubscribe(&unsub);
    assert!(adapter.subscriptions_instrument_close.is_empty());
}

// --------------------------------------------------------------------------------------------
// Request handler tests
// --------------------------------------------------------------------------------------------

#[rstest]
fn test_request_data(
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let recorder = Rc::new(RefCell::new(Vec::<DataCommand>::new()));
    let client = Box::new(MockDataClient::new_with_recorder(
        clock,
        cache,
        client_id,
        Some(venue),
        Some(Rc::clone(&recorder)),
    ));
    let adapter = DataClientAdapter::new(client_id, Some(venue), false, false, client);

    let data_type = DataType::new("ReqType", None, None);
    let req = RequestCustomData {
        client_id,
        data_type,
        start: None,
        end: None,
        limit: None,
        request_id: UUID4::new(),
        ts_init: UnixNanos::default(),
        params: None,
    };
    adapter.request_data(req.clone()).unwrap();

    let rec = recorder.borrow();
    assert_eq!(rec.len(), 1);
    assert_eq!(rec[0], DataCommand::Request(RequestCommand::Data(req)));
}

#[rstest]
fn test_request_instrument(
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let recorder = Rc::new(RefCell::new(Vec::<DataCommand>::new()));
    let client = Box::new(MockDataClient::new_with_recorder(
        clock,
        cache,
        client_id,
        Some(venue),
        Some(Rc::clone(&recorder)),
    ));
    let adapter = DataClientAdapter::new(client_id, Some(venue), false, false, client);

    let inst_id = audusd_sim().id;
    let req = RequestInstrument::new(
        inst_id,
        None,
        None,
        Some(client_id),
        UUID4::new(),
        UnixNanos::default(),
        None,
    );
    adapter.request_instrument(req.clone()).unwrap();

    let rec = recorder.borrow();
    assert_eq!(rec.len(), 1);
    assert_eq!(
        rec[0],
        DataCommand::Request(RequestCommand::Instrument(req))
    );
}

#[rstest]
fn test_request_instruments(
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    // record request commands sent to the client
    let recorder = Rc::new(RefCell::new(Vec::<DataCommand>::new()));
    let client = Box::new(MockDataClient::new_with_recorder(
        clock,
        cache,
        client_id,
        Some(venue),
        Some(Rc::clone(&recorder)),
    ));
    let adapter = DataClientAdapter::new(client_id, Some(venue), false, false, client);

    let req = RequestInstruments::new(
        None,
        None,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
    );
    adapter.request_instruments(req.clone()).unwrap();

    let rec = recorder.borrow();
    assert_eq!(rec.len(), 1);
    assert_eq!(
        rec[0],
        DataCommand::Request(RequestCommand::Instruments(req))
    );
}

#[rstest]
fn test_request_book_snapshot(
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let recorder = Rc::new(RefCell::new(Vec::<DataCommand>::new()));
    let client = Box::new(MockDataClient::new_with_recorder(
        clock,
        cache,
        client_id,
        Some(venue),
        Some(Rc::clone(&recorder)),
    ));
    let adapter = DataClientAdapter::new(client_id, Some(venue), false, false, client);

    let inst_id = audusd_sim().id;
    let req = RequestBookSnapshot::new(
        inst_id,
        None, // depth
        Some(client_id),
        UUID4::new(),
        UnixNanos::default(),
        None, // params
    );
    adapter.request_book_snapshot(req.clone()).unwrap();

    let rec = recorder.borrow();
    assert_eq!(rec.len(), 1);
    assert_eq!(
        rec[0],
        DataCommand::Request(RequestCommand::BookSnapshot(req))
    );
}

#[rstest]
fn test_request_quotes(
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let recorder = Rc::new(RefCell::new(Vec::<DataCommand>::new()));
    let client = Box::new(MockDataClient::new_with_recorder(
        clock,
        cache,
        client_id,
        Some(venue),
        Some(Rc::clone(&recorder)),
    ));
    let adapter = DataClientAdapter::new(client_id, Some(venue), false, false, client);

    let inst_id = audusd_sim().id;
    let req = RequestQuotes::new(
        inst_id,
        None,
        None,
        None,
        Some(client_id),
        UUID4::new(),
        UnixNanos::default(),
        None,
    );
    adapter.request_quotes(req.clone()).unwrap();

    let rec = recorder.borrow();
    assert_eq!(rec.len(), 1);
    assert_eq!(rec[0], DataCommand::Request(RequestCommand::Quotes(req)));
}

#[rstest]
fn test_request_trades(
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let recorder = Rc::new(RefCell::new(Vec::<DataCommand>::new()));
    let client = Box::new(MockDataClient::new_with_recorder(
        clock,
        cache,
        client_id,
        Some(venue),
        Some(Rc::clone(&recorder)),
    ));
    let adapter = DataClientAdapter::new(client_id, Some(venue), false, false, client);

    let inst_id = audusd_sim().id;
    let req = RequestTrades::new(
        inst_id,
        None,
        None,
        None,
        Some(client_id),
        UUID4::new(),
        UnixNanos::default(),
        None,
    );
    adapter.request_trades(req.clone()).unwrap();

    let rec = recorder.borrow();
    assert_eq!(rec.len(), 1);
    assert_eq!(rec[0], DataCommand::Request(RequestCommand::Trades(req)));
}

#[rstest]
fn test_request_funding_rates(
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let recorder = Rc::new(RefCell::new(Vec::<DataCommand>::new()));
    let client = Box::new(MockDataClient::new_with_recorder(
        clock,
        cache,
        client_id,
        Some(venue),
        Some(Rc::clone(&recorder)),
    ));
    let adapter = DataClientAdapter::new(client_id, Some(venue), false, false, client);

    let inst_id = audusd_sim().id;
    let req = RequestFundingRates::new(
        inst_id,
        None,
        None,
        None,
        Some(client_id),
        UUID4::new(),
        UnixNanos::default(),
        None,
    );
    adapter.request_funding_rates(req.clone()).unwrap();

    let rec = recorder.borrow();
    assert_eq!(rec.len(), 1);
    assert_eq!(
        rec[0],
        DataCommand::Request(RequestCommand::FundingRates(req))
    );
}

#[rstest]
fn test_request_bars(
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let recorder = Rc::new(RefCell::new(Vec::<DataCommand>::new()));
    let client = Box::new(MockDataClient::new_with_recorder(
        clock,
        cache,
        client_id,
        Some(venue),
        Some(Rc::clone(&recorder)),
    ));
    let adapter = DataClientAdapter::new(client_id, Some(venue), false, false, client);

    let bar_type: BarType = "AUDUSD.SIM-1-MINUTE-LAST-INTERNAL".into();
    let req = RequestBars::new(
        bar_type,
        None,
        None,
        None,
        Some(client_id),
        UUID4::new(),
        UnixNanos::default(),
        None,
    );
    adapter.request_bars(req.clone()).unwrap();

    let rec = recorder.borrow();
    assert_eq!(rec.len(), 1);
    assert_eq!(rec[0], DataCommand::Request(RequestCommand::Bars(req)));
}

#[rstest]
fn test_request_order_book_depth(
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let recorder = Rc::new(RefCell::new(Vec::<DataCommand>::new()));
    let client = Box::new(MockDataClient::new_with_recorder(
        clock,
        cache,
        client_id,
        Some(venue),
        Some(Rc::clone(&recorder)),
    ));
    let adapter = DataClientAdapter::new(client_id, Some(venue), false, false, client);

    let inst_id = audusd_sim().id;
    let req = RequestBookDepth::new(
        inst_id,
        None,
        None,
        None,
        Some(NonZeroUsize::new(10).unwrap()),
        Some(client_id),
        UUID4::new(),
        UnixNanos::default(),
        None,
    );
    adapter.request_book_depth(req.clone()).unwrap();

    let rec = recorder.borrow();
    assert_eq!(rec.len(), 1);
    assert_eq!(rec[0], DataCommand::Request(RequestCommand::BookDepth(req)));
}

// ------------------------------------------------------------------------------------------------
// DeFi subscription tests
// ------------------------------------------------------------------------------------------------

#[cfg(feature = "defi")]
#[rstest]
fn test_defi_blocks_subscription(
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let client = Box::new(MockDataClient::new(clock, cache, client_id, Some(venue)));
    let mut adapter = DataClientAdapter::new(client_id, Some(venue), false, false, client);

    let blockchain = Blockchain::Ethereum;

    let sub = DefiSubscribeCommand::Blocks(SubscribeBlocks {
        chain: blockchain,
        client_id: Some(client_id),
        command_id: UUID4::new(),
        ts_init: UnixNanos::default(),
        params: None,
    });
    adapter.execute_defi_subscribe(sub.clone());
    assert!(adapter.subscriptions_blocks.contains(&blockchain));

    // Idempotency check
    adapter.execute_defi_subscribe(sub.clone());
    assert_eq!(adapter.subscriptions_blocks.len(), 1);

    let unsub = DefiUnsubscribeCommand::Blocks(UnsubscribeBlocks {
        chain: blockchain,
        client_id: Some(client_id),
        command_id: UUID4::new(),
        ts_init: UnixNanos::default(),
        params: None,
    });
    adapter.execute_defi_unsubscribe(&unsub);
    assert!(!adapter.subscriptions_blocks.contains(&blockchain));
}

#[cfg(feature = "defi")]
#[rstest]
fn test_defi_pool_subscription(
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let recorder = Rc::new(RefCell::new(Vec::new()));
    let client = Box::new(MockDataClient::new_with_recorder(
        clock,
        cache,
        client_id,
        Some(venue),
        Some(Rc::clone(&recorder)),
    ));
    let mut adapter = DataClientAdapter::new(client_id, Some(venue), false, false, client);
    let instrument_id =
        InstrumentId::from("0x11b815efB8f581194ae79006d24E0d814B7697F6.Arbitrum:UniswapV3");
    let subscribe = DefiSubscribeCommand::Pool(SubscribePool {
        instrument_id,
        client_id: Some(client_id),
        command_id: UUID4::new(),
        ts_init: UnixNanos::from(1),
        params: None,
    });
    let unsubscribe = DefiUnsubscribeCommand::Pool(UnsubscribePool {
        instrument_id,
        client_id: Some(client_id),
        command_id: UUID4::new(),
        ts_init: UnixNanos::from(2),
        params: None,
    });

    adapter.execute_defi_subscribe(subscribe.clone());
    adapter.execute_defi_unsubscribe(&unsubscribe);

    assert!(!adapter.subscriptions_pools.contains(&instrument_id));
    assert_eq!(
        recorder.borrow().as_slice(),
        &[
            DataCommand::DefiSubscribe(subscribe),
            DataCommand::DefiUnsubscribe(unsubscribe),
        ]
    );
}

#[cfg(feature = "defi")]
#[rstest]
fn test_defi_blocks_release_after_final_owner(
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let recorder = Rc::new(RefCell::new(Vec::new()));
    let client = Box::new(MockDataClient::new_with_recorder(
        clock,
        cache,
        client_id,
        Some(venue),
        Some(Rc::clone(&recorder)),
    ));
    let mut adapter = DataClientAdapter::new(client_id, Some(venue), false, false, client);
    let chain = Blockchain::Arbitrum;
    let first_params = Params::new([("owner".to_string(), serde_json::json!(1))].into());

    for params in [Some(first_params.clone()), None] {
        adapter.execute_defi_subscribe(DefiSubscribeCommand::Blocks(SubscribeBlocks {
            chain,
            client_id: Some(client_id),
            command_id: UUID4::new(),
            ts_init: UnixNanos::default(),
            params,
        }));
    }
    assert_eq!(recorder.borrow().len(), 1);

    adapter.execute_defi_unsubscribe(&DefiUnsubscribeCommand::Blocks(UnsubscribeBlocks {
        chain,
        client_id: Some(client_id),
        command_id: UUID4::new(),
        ts_init: UnixNanos::from(1),
        params: None,
    }));
    assert_eq!(recorder.borrow().len(), 1);
    assert!(adapter.subscriptions_blocks.contains(&chain));

    adapter.execute_defi_unsubscribe(&DefiUnsubscribeCommand::Blocks(UnsubscribeBlocks {
        chain,
        client_id: Some(client_id),
        command_id: UUID4::new(),
        ts_init: UnixNanos::from(2),
        params: None,
    }));

    let recorded = recorder.borrow();
    let [
        DataCommand::DefiSubscribe(DefiSubscribeCommand::Blocks(_)),
        DataCommand::DefiUnsubscribe(DefiUnsubscribeCommand::Blocks(command)),
    ] = recorded.as_slice()
    else {
        panic!("expected one subscribe and one final unsubscribe, was {recorded:?}");
    };
    assert_eq!(command.chain, chain);
    assert_eq!(command.client_id, Some(client_id));
    assert_eq!(command.ts_init, UnixNanos::from(2));
    assert_eq!(command.params.as_ref(), Some(&first_params));
    assert!(!adapter.subscriptions_blocks.contains(&chain));
}

#[cfg(feature = "defi")]
#[rstest]
fn test_defi_blocks_subscription_retries_after_client_failure(
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let recorder = Rc::new(RefCell::new(Vec::new()));
    let client = Box::new(
        MockDataClient::new_with_recorder(
            clock,
            cache,
            client_id,
            Some(venue),
            Some(Rc::clone(&recorder)),
        )
        .with_blocks_subscribe_failure(),
    );
    let mut adapter = DataClientAdapter::new(client_id, Some(venue), false, false, client);
    let chain = Blockchain::Arbitrum;
    let subscribe = |command_id| {
        DefiSubscribeCommand::Blocks(SubscribeBlocks {
            chain,
            client_id: Some(client_id),
            command_id,
            ts_init: UnixNanos::default(),
            params: None,
        })
    };

    adapter.execute_defi_subscribe(subscribe(UUID4::new()));
    assert!(!adapter.subscriptions_blocks.contains(&chain));
    assert!(recorder.borrow().is_empty());

    adapter.execute_defi_subscribe(subscribe(UUID4::new()));
    assert!(adapter.subscriptions_blocks.contains(&chain));
    assert_eq!(recorder.borrow().len(), 1);
    recorder.borrow_mut().clear();

    adapter.execute_defi_unsubscribe(&DefiUnsubscribeCommand::Blocks(UnsubscribeBlocks {
        chain,
        client_id: Some(client_id),
        command_id: UUID4::new(),
        ts_init: UnixNanos::default(),
        params: None,
    }));

    assert!(!adapter.subscriptions_blocks.contains(&chain));
    assert_eq!(recorder.borrow().len(), 1);
}

#[cfg(feature = "defi")]
#[rstest]
#[case::retry(false)]
#[case::reacquire(true)]
fn test_defi_blocks_unsubscription_retries_after_client_failure(
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
    #[case] reacquire: bool,
) {
    let recorder = Rc::new(RefCell::new(Vec::new()));
    let client = Box::new(
        MockDataClient::new_with_recorder(
            clock,
            cache,
            client_id,
            Some(venue),
            Some(Rc::clone(&recorder)),
        )
        .with_blocks_unsubscribe_failure(),
    );
    let mut adapter = DataClientAdapter::new(client_id, Some(venue), false, false, client);
    let chain = Blockchain::Arbitrum;
    let params = Params::new([("route".to_string(), serde_json::json!(41))].into());
    adapter.execute_defi_subscribe(DefiSubscribeCommand::Blocks(SubscribeBlocks {
        chain,
        client_id: Some(client_id),
        command_id: UUID4::new(),
        ts_init: UnixNanos::default(),
        params: Some(params.clone()),
    }));
    recorder.borrow_mut().clear();

    let unsubscribe = |command_id, ts_init| {
        DefiUnsubscribeCommand::Blocks(UnsubscribeBlocks {
            chain,
            client_id: Some(client_id),
            command_id,
            ts_init,
            params: None,
        })
    };
    adapter.execute_defi_unsubscribe(&unsubscribe(UUID4::new(), UnixNanos::from(1)));

    assert!(adapter.subscriptions_blocks.contains(&chain));
    assert!(recorder.borrow().is_empty());

    if reacquire {
        adapter.execute_defi_subscribe(DefiSubscribeCommand::Blocks(SubscribeBlocks {
            chain,
            client_id: Some(client_id),
            command_id: UUID4::new(),
            ts_init: UnixNanos::from(2),
            params: None,
        }));
        assert!(recorder.borrow().is_empty());
    }
    let retry = unsubscribe(UUID4::new(), UnixNanos::from(2));
    adapter.execute_defi_unsubscribe(&retry);

    assert!(!adapter.subscriptions_blocks.contains(&chain));
    let DefiUnsubscribeCommand::Blocks(mut expected) = retry else {
        unreachable!()
    };
    expected.params = Some(params);
    let recorded = recorder.borrow();
    let [DataCommand::DefiUnsubscribe(DefiUnsubscribeCommand::Blocks(actual))] =
        recorded.as_slice()
    else {
        panic!("expected one successful blocks unsubscribe");
    };
    assert_eq!(
        serde_json::to_value(actual).unwrap(),
        serde_json::to_value(expected).unwrap()
    );
}

#[cfg(feature = "defi")]
#[rstest]
fn test_defi_pool_swaps_subscription(
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let client = Box::new(MockDataClient::new(clock, cache, client_id, Some(venue)));
    let mut adapter = DataClientAdapter::new(client_id, Some(venue), false, false, client);

    let instrument_id =
        InstrumentId::from("0x11b815efB8f581194ae79006d24E0d814B7697F6.Arbitrum:UniswapV3");

    let sub = DefiSubscribeCommand::PoolSwaps(SubscribePoolSwaps {
        instrument_id,
        client_id: Some(client_id),
        command_id: UUID4::new(),
        ts_init: UnixNanos::default(),
        params: None,
    });
    adapter.execute_defi_subscribe(sub.clone());
    assert!(adapter.subscriptions_pool_swaps.contains(&instrument_id));

    // Idempotency check
    adapter.execute_defi_subscribe(sub.clone());
    assert_eq!(adapter.subscriptions_pool_swaps.len(), 1);

    let unsub = DefiUnsubscribeCommand::PoolSwaps(UnsubscribePoolSwaps {
        instrument_id,
        client_id: Some(client_id),
        command_id: UUID4::new(),
        ts_init: UnixNanos::default(),
        params: None,
    });
    adapter.execute_defi_unsubscribe(&unsub);
    assert!(!adapter.subscriptions_pool_swaps.contains(&instrument_id));
}

#[cfg(feature = "defi")]
#[rstest]
fn test_defi_blocks_unsubscribe_noop(
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let client = Box::new(MockDataClient::new(clock, cache, client_id, Some(venue)));
    let mut adapter = DataClientAdapter::new(client_id, Some(venue), false, false, client);

    // Unsubscribe without prior subscribe should be no-op
    let blockchain = Blockchain::Ethereum;
    let unsub = DefiUnsubscribeCommand::Blocks(UnsubscribeBlocks {
        chain: blockchain,
        client_id: Some(client_id),
        command_id: UUID4::new(),
        ts_init: UnixNanos::default(),
        params: None,
    });
    adapter.execute_defi_unsubscribe(&unsub);
    assert!(!adapter.subscriptions_blocks.contains(&blockchain));
    assert!(adapter.subscriptions_blocks.is_empty());
}

#[cfg(feature = "defi")]
#[rstest]
fn test_defi_blocks_unsubscribe_idempotent(
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let client = Box::new(MockDataClient::new(clock, cache, client_id, Some(venue)));
    let mut adapter = DataClientAdapter::new(client_id, Some(venue), false, false, client);

    // Subscribe then unsubscribe twice
    let blockchain = Blockchain::Ethereum;
    let sub = DefiSubscribeCommand::Blocks(SubscribeBlocks {
        chain: blockchain,
        client_id: Some(client_id),
        command_id: UUID4::new(),
        ts_init: UnixNanos::default(),
        params: None,
    });
    adapter.execute_defi_subscribe(sub.clone());

    let unsub = DefiUnsubscribeCommand::Blocks(UnsubscribeBlocks {
        chain: blockchain,
        client_id: Some(client_id),
        command_id: UUID4::new(),
        ts_init: UnixNanos::default(),
        params: None,
    });
    adapter.execute_defi_unsubscribe(&unsub);
    adapter.execute_defi_unsubscribe(&unsub);

    // Expect adapter state cleared and no panic on second unsubscribe
    assert!(!adapter.subscriptions_blocks.contains(&blockchain));
}

#[cfg(feature = "defi")]
#[rstest]
fn test_defi_pool_swaps_unsubscribe_noop(
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let client = Box::new(MockDataClient::new(clock, cache, client_id, Some(venue)));
    let mut adapter = DataClientAdapter::new(client_id, Some(venue), false, false, client);

    let instrument_id =
        InstrumentId::from("0x11b815efB8f581194ae79006d24E0d814B7697F6.Arbitrum:UniswapV3");

    // Unsubscribe without prior subscribe should be no-op
    let unsub = DefiUnsubscribeCommand::PoolSwaps(UnsubscribePoolSwaps {
        instrument_id,
        client_id: Some(client_id),
        command_id: UUID4::new(),
        ts_init: UnixNanos::default(),
        params: None,
    });
    adapter.execute_defi_unsubscribe(&unsub);
    assert!(!adapter.subscriptions_pool_swaps.contains(&instrument_id));
    assert!(adapter.subscriptions_pool_swaps.is_empty());
}

#[cfg(feature = "defi")]
#[rstest]
fn test_defi_pool_swaps_unsubscribe_idempotent(
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let client = Box::new(MockDataClient::new(clock, cache, client_id, Some(venue)));
    let mut adapter = DataClientAdapter::new(client_id, Some(venue), false, false, client);

    let instrument_id =
        InstrumentId::from("0x11b815efB8f581194ae79006d24E0d814B7697F6.Arbitrum:UniswapV3");

    // Subscribe then unsubscribe twice
    let sub = DefiSubscribeCommand::PoolSwaps(SubscribePoolSwaps {
        instrument_id,
        client_id: Some(client_id),
        command_id: UUID4::new(),
        ts_init: UnixNanos::default(),
        params: None,
    });
    adapter.execute_defi_subscribe(sub.clone());

    let unsub = DefiUnsubscribeCommand::PoolSwaps(UnsubscribePoolSwaps {
        instrument_id,
        client_id: Some(client_id),
        command_id: UUID4::new(),
        ts_init: UnixNanos::default(),
        params: None,
    });
    adapter.execute_defi_unsubscribe(&unsub);
    adapter.execute_defi_unsubscribe(&unsub);

    // Expect adapter state cleared and no panic on second unsubscribe
    assert!(!adapter.subscriptions_pool_swaps.contains(&instrument_id));
}

#[rstest]
#[case("Instrument")]
#[case("BookDeltas")]
#[case("BookDepth")]
#[case("MarkPrices")]
#[case("IndexPrices")]
#[case("FundingRates")]
#[case("InstrumentStatus")]
#[case("InstrumentClose")]
#[case("OptionGreeks")]
fn test_instrument_list_adapter_ownership_and_list_dispatch(
    #[case] family: &str,
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let recorder = Rc::new(RefCell::new(Vec::new()));

    let client = Box::new(MockDataClient::new_with_recorder(
        clock,
        cache,
        client_id,
        Some(venue),
        Some(Rc::clone(&recorder)),
    ));
    let mut adapter = DataClientAdapter::new(client_id, Some(venue), false, false, client);
    let first = InstrumentId::from("AUD/USD.SIM");
    let second = InstrumentId::from("EUR/USD.SIM");
    let third = InstrumentId::from("GBP/USD.SIM");
    let mut fields = serde_json::json!({"instrument_ids": [second, first, second], "client_id": client_id,
        "venue": venue, "command_id": UUID4::new(), "ts_init": 41, "correlation_id": UUID4::new(), "params": {"owner": 59}});

    if family.starts_with("Book") {
        fields["book_type"] = serde_json::json!(BookType::L2_MBP);
        fields["depth"] = serde_json::json!(7);
        fields["managed"] = serde_json::json!(true);
    }

    let command: SubscribeCommand =
        serde_json::from_value(serde_json::json!({family: fields})).unwrap();
    adapter.execute_subscribe(command.clone());
    adapter.execute_subscribe(command.clone());
    let mut owner_fields = fields.clone();
    owner_fields["instrument_ids"] = serde_json::json!([second, third]);
    owner_fields["command_id"] = serde_json::json!(UUID4::new());
    owner_fields["params"] = serde_json::json!({"owner": 61});
    let owner: SubscribeCommand =
        serde_json::from_value(serde_json::json!({family: owner_fields})).unwrap();
    adapter.execute_subscribe(owner.clone());
    let release_id = UUID4::new();
    let release = command
        .clone()
        .into_unsubscribe(release_id, UnixNanos::from(43), None);
    adapter.execute_unsubscribe(&release);
    let after_first_release = recorder.borrow().len();
    let final_release_id = UUID4::new();
    adapter.execute_unsubscribe(&owner.clone().into_unsubscribe(
        final_release_id,
        UnixNanos::from(47),
        None,
    ));
    let recorded = recorder.borrow();

    let actual = recorded
        .iter()
        .map(|command| match command {
            DataCommand::Subscribe(command) => (true, serde_json::to_value(command).unwrap()),
            DataCommand::Unsubscribe(command) => (false, serde_json::to_value(command).unwrap()),
            _ => panic!("unexpected command"),
        })
        .collect::<Vec<_>>();

    let expected = vec![
        (true, serde_json::to_value(&command).unwrap()),
        (
            true,
            serde_json::to_value(owner.with_instrument_ids(third).unwrap()).unwrap(),
        ),
        (
            false,
            serde_json::to_value(
                command
                    .with_instrument_ids(first)
                    .unwrap()
                    .into_unsubscribe(release_id, UnixNanos::from(43), None),
            )
            .unwrap(),
        ),
        (
            false,
            serde_json::to_value(
                command
                    .with_instrument_ids(second)
                    .unwrap()
                    .into_unsubscribe(final_release_id, UnixNanos::from(47), None),
            )
            .unwrap(),
        ),
        (
            false,
            serde_json::to_value(owner.with_instrument_ids(third).unwrap().into_unsubscribe(
                final_release_id,
                UnixNanos::from(47),
                None,
            ))
            .unwrap(),
        ),
    ];

    let active = match family {
        "Instrument" => &adapter.subscriptions_instrument,
        "BookDeltas" => &adapter.subscriptions_book_deltas,
        "BookDepth" => &adapter.subscriptions_book_depth,
        "MarkPrices" => &adapter.subscriptions_mark_prices,
        "IndexPrices" => &adapter.subscriptions_index_prices,
        "FundingRates" => &adapter.subscriptions_funding_rates,
        "InstrumentStatus" => &adapter.subscriptions_instrument_status,
        "InstrumentClose" => &adapter.subscriptions_instrument_close,
        "OptionGreeks" => &adapter.subscriptions_option_greeks,
        _ => unreachable!(),
    };

    assert_eq!(after_first_release, 3);
    assert_eq!(actual, expected);
    assert_eq!(active, &AHashSet::new());
}

#[rstest]
fn test_bar_list_failed_release_retries_original_member_metadata(
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let recorder = Rc::new(RefCell::new(Vec::new()));
    let client = MockDataClient::new_with_recorder(
        clock,
        cache,
        client_id,
        Some(venue),
        Some(Rc::clone(&recorder)),
    )
    .with_bars_unsubscribe_failure();
    let mut adapter =
        DataClientAdapter::new(client_id, Some(venue), false, false, Box::new(client));
    let first = BarType::from("AUD/USD.SIM-1-MINUTE-LAST-EXTERNAL");
    let second = BarType::from("AUD/USD.SIM-5-MINUTE-LAST-EXTERNAL");

    let subscribe = SubscribeBars::new(
        [second, first],
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::from(23),
        Some(UUID4::new()),
        Some(serde_json::from_value(serde_json::json!({"route": 37})).unwrap()),
    );
    adapter.execute_subscribe(SubscribeCommand::Bars(subscribe.clone()));

    let release = UnsubscribeBars::new(
        [first, second],
        Some(ClientId::from("FALLBACK")),
        Some(Venue::from("OTHER")),
        UUID4::new(),
        UnixNanos::from(31),
        Some(UUID4::new()),
        Some(serde_json::from_value(serde_json::json!({"route": 59})).unwrap()),
    );
    adapter.execute_unsubscribe(&UnsubscribeCommand::Bars(release.clone()));
    assert_eq!(
        adapter.subscriptions_bars,
        AHashSet::from_iter([first, second])
    );
    assert_eq!(recorder.borrow().len(), 1);

    let retry = UnsubscribeBars {
        command_id: UUID4::new(),
        ts_init: UnixNanos::from(47),
        correlation_id: Some(UUID4::new()),
        ..release.with_bar_types([first, second])
    };

    adapter.execute_unsubscribe(&UnsubscribeCommand::Bars(retry.clone()));

    let actual = recorder
        .borrow()
        .iter()
        .map(|cmd| match cmd {
            DataCommand::Subscribe(cmd) => serde_json::to_value(cmd).unwrap(),
            DataCommand::Unsubscribe(cmd) => serde_json::to_value(cmd).unwrap(),
            _ => panic!("unexpected command"),
        })
        .collect::<Vec<_>>();

    let expected = vec![
        serde_json::to_value(SubscribeCommand::Bars(subscribe.clone())).unwrap(),
        serde_json::to_value(
            SubscribeCommand::Bars(subscribe.with_bar_types([first, second])).into_unsubscribe(
                retry.command_id,
                retry.ts_init,
                retry.correlation_id,
            ),
        )
        .unwrap(),
    ];
    assert_eq!(actual, expected);
    assert_eq!(adapter.subscriptions_bars, AHashSet::new());
}

#[rstest]
#[case::instruments(false)]
#[case::full_bar_types(true)]
fn test_partial_lists_intersect_acceptance_and_retry_saved_release(
    #[case] bars: bool,
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let recorder = Rc::new(RefCell::new(Vec::new()));
    let first = InstrumentId::from("AUD/USD.SIM");
    let second = InstrumentId::from("GBP/USD.SIM");
    let foreign = InstrumentId::from("FOREIGN.SIM");
    let first_bar = BarType::from("AUD/USD.SIM-1-MINUTE-LAST-EXTERNAL");
    let second_bar = BarType::from("AUD/USD.SIM-5-MINUTE-LAST-EXTERNAL");
    let foreign_bar = BarType::from("AUD/USD.SIM-1-HOUR-LAST-EXTERNAL");
    let outcome = if bars {
        MockSubscribeFailure::BarsPartial([first_bar, first_bar, foreign_bar])
    } else {
        MockSubscribeFailure::MarkPricesPartial([first, first, foreign])
    };

    let client = MockDataClient::new_with_recorder(
        clock,
        cache,
        client_id,
        Some(venue),
        Some(Rc::clone(&recorder)),
    )
    .with_subscribe_failure(outcome)
    .with_unsubscribe_failure(outcome);
    let mut adapter =
        DataClientAdapter::new(client_id, Some(venue), false, false, Box::new(client));
    let params: Params =
        serde_json::from_value(serde_json::json!({"route": 17, "nested": {"value": 23}})).unwrap();

    let command = if bars {
        SubscribeCommand::Bars(SubscribeBars::new(
            [second_bar, first_bar],
            Some(client_id),
            Some(venue),
            UUID4::new(),
            UnixNanos::from(31),
            Some(UUID4::new()),
            Some(params),
        ))
    } else {
        SubscribeCommand::MarkPrices(SubscribeMarkPrices::new(
            [second, first],
            Some(client_id),
            Some(venue),
            UUID4::new(),
            UnixNanos::from(31),
            Some(UUID4::new()),
            Some(params),
        ))
    };

    adapter.execute_subscribe(command.clone());
    assert_partial_members(&adapter, bars, first, first_bar);

    // Replay acquires only the missing member, under the same acquisition identity
    adapter.execute_subscribe(command.clone());
    let mut release =
        command
            .clone()
            .into_unsubscribe(UUID4::new(), UnixNanos::from(37), Some(UUID4::new()));

    match &mut release {
        UnsubscribeCommand::Bars(command) => {
            command.params = None;
            command.client_id = Some(ClientId::from("FALLBACK"));
        }
        UnsubscribeCommand::MarkPrices(command) => {
            command.params = None;
            command.client_id = Some(ClientId::from("FALLBACK"));
        }
        _ => unreachable!(),
    }

    adapter.execute_unsubscribe(&release);
    assert_partial_members(&adapter, bars, second, second_bar);
    let retry = if bars {
        release.with_bar_types(second_bar).unwrap()
    } else {
        release.with_instrument_ids(second).unwrap()
    };

    adapter.execute_unsubscribe(&retry);
    let remaining = if bars {
        adapter.subscriptions_bars.len()
    } else {
        adapter.subscriptions_mark_prices.len()
    };

    let missing = if bars {
        command.with_bar_types(second_bar).unwrap()
    } else {
        command.with_instrument_ids(second).unwrap()
    };

    let expected = vec![
        serde_json::to_value(&command).unwrap(),
        serde_json::to_value(&missing).unwrap(),
        serde_json::to_value(command.into_unsubscribe(
            release.command_id(),
            release.ts_init(),
            release.correlation_id(),
        ))
        .unwrap(),
        serde_json::to_value(missing.into_unsubscribe(
            retry.command_id(),
            retry.ts_init(),
            retry.correlation_id(),
        ))
        .unwrap(),
    ];

    let actual: Vec<_> = recorder
        .borrow()
        .iter()
        .map(|command| match command {
            DataCommand::Subscribe(command) => serde_json::to_value(command).unwrap(),
            DataCommand::Unsubscribe(command) => serde_json::to_value(command).unwrap(),
            _ => unreachable!(),
        })
        .collect();

    assert_eq!(remaining, 0);
    assert_eq!(actual, expected);
}

fn assert_partial_members(
    adapter: &DataClientAdapter,
    bars: bool,
    id: InstrumentId,
    bar_type: BarType,
) {
    if bars {
        assert_eq!(adapter.subscriptions_bars, AHashSet::from_iter([bar_type]));
        assert_eq!(adapter.subscriptions_mark_prices, AHashSet::new());
    } else {
        assert_eq!(adapter.subscriptions_mark_prices, AHashSet::from_iter([id]));
        assert_eq!(adapter.subscriptions_bars, AHashSet::new());
    }
}

#[rstest]
fn test_partial_release_cannot_remove_reported_member_still_owned_by_peer(
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let recorder = Rc::new(RefCell::new(Vec::new()));
    let first = InstrumentId::from("AUD/USD.SIM");
    let second = InstrumentId::from("GBP/USD.SIM");
    let foreign = InstrumentId::from("FOREIGN.SIM");
    let client = MockDataClient::new_with_recorder(
        clock,
        cache,
        client_id,
        Some(venue),
        Some(Rc::clone(&recorder)),
    )
    .with_unsubscribe_failure(MockSubscribeFailure::MarkPricesPartial([
        first, first, foreign,
    ]));
    let mut adapter =
        DataClientAdapter::new(client_id, Some(venue), false, false, Box::new(client));
    let params: Params = serde_json::from_value(serde_json::json!({"route": 17})).unwrap();

    let original = SubscribeMarkPrices::new(
        [first, second],
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::from(23),
        Some(UUID4::new()),
        Some(params),
    );
    adapter.execute_subscribe(SubscribeCommand::MarkPrices(original.clone()));

    let peer = SubscribeMarkPrices {
        command_id: UUID4::new(),
        params: Some(serde_json::from_value(serde_json::json!({"route": 31})).unwrap()),
        ..original.with_instrument_ids(first)
    };

    adapter.execute_subscribe(SubscribeCommand::MarkPrices(peer.clone()));
    let release = SubscribeCommand::MarkPrices(original.clone()).into_unsubscribe(
        UUID4::new(),
        UnixNanos::from(37),
        None,
    );
    adapter.execute_unsubscribe(&release);
    assert_eq!(
        adapter.subscriptions_mark_prices,
        AHashSet::from_iter([first, second])
    );
    let retry = release.with_instrument_ids(second).unwrap();
    adapter.execute_unsubscribe(&retry);
    assert_eq!(
        adapter.subscriptions_mark_prices,
        AHashSet::from_iter([first])
    );
    let final_release = SubscribeCommand::MarkPrices(peer).into_unsubscribe(
        UUID4::new(),
        UnixNanos::from(41),
        None,
    );
    adapter.execute_unsubscribe(&final_release);
    let second_release = SubscribeCommand::MarkPrices(original.with_instrument_ids(second))
        .into_unsubscribe(
            release.command_id(),
            release.ts_init(),
            release.correlation_id(),
        );
    let expected = vec![
        serde_json::to_value(SubscribeCommand::MarkPrices(original.clone())).unwrap(),
        serde_json::to_value(&second_release).unwrap(),
        serde_json::to_value(second_release).unwrap(),
        serde_json::to_value(
            SubscribeCommand::MarkPrices(original.with_instrument_ids(first)).into_unsubscribe(
                final_release.command_id(),
                final_release.ts_init(),
                final_release.correlation_id(),
            ),
        )
        .unwrap(),
    ];

    let actual: Vec<_> = recorder
        .borrow()
        .iter()
        .map(|command| match command {
            DataCommand::Subscribe(command) => serde_json::to_value(command).unwrap(),
            DataCommand::Unsubscribe(command) => serde_json::to_value(command).unwrap(),
            _ => unreachable!(),
        })
        .collect();

    assert_eq!(actual, expected);
    assert_eq!(adapter.subscriptions_mark_prices, AHashSet::new());
}
