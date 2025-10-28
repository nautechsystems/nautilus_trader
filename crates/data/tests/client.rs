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

mod common;
use std::{cell::RefCell, num::NonZeroUsize, rc::Rc};

use common::mocks::MockDataClient;
use nautilus_common::{
    cache::Cache,
    clock::TestClock,
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
            RequestInstrument,
            RequestInstruments,
            RequestQuotes,
            RequestTrades,
            // Subscription commands
            SubscribeBars,
            SubscribeBookDeltas,
            SubscribeBookDepth10,
            SubscribeBookSnapshots,
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
            UnsubscribeBookDepth10,
            UnsubscribeBookSnapshots,
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
use nautilus_core::{UUID4, UnixNanos};
use nautilus_data::client::DataClientAdapter;
use nautilus_model::{
    data::{BarType, DataType},
    enums::BookType,
    identifiers::{ClientId, InstrumentId, Venue},
    instruments::stubs::audusd_sim,
};
use rstest::{fixture, rstest};
#[cfg(feature = "defi")]
use {
    nautilus_common::messages::defi::{
        DefiSubscribeCommand, DefiUnsubscribeCommand, SubscribeBlocks, SubscribePoolSwaps,
        UnsubscribeBlocks, UnsubscribePoolSwaps,
    },
    nautilus_model::defi::Blockchain,
};

#[fixture]
fn clock() -> Rc<RefCell<TestClock>> {
    Rc::new(RefCell::new(TestClock::new()))
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
    Venue::default()
}

// --------------------------------------------------------------------------------------------
// Subscription handler tests
// --------------------------------------------------------------------------------------------

#[rstest]
fn test_custom_data_subscription(
    clock: Rc<RefCell<TestClock>>,
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
    let data_type = DataType::new("MyType", None);

    let sub = SubscribeCommand::Data(SubscribeCustomData::new(
        Some(client_id),
        Some(venue),
        data_type.clone(),
        UUID4::new(),
        UnixNanos::default(),
        None,
    ));
    adapter.execute_subscribe(&sub);
    assert!(adapter.subscriptions_custom.contains(&data_type));

    // Idempotency check
    adapter.execute_subscribe(&sub);
    assert_eq!(adapter.subscriptions_custom.len(), 1);

    let unsub = UnsubscribeCommand::Data(UnsubscribeCustomData::new(
        Some(client_id),
        Some(venue),
        data_type.clone(),
        UUID4::new(),
        UnixNanos::default(),
        None,
    ));
    adapter.execute_unsubscribe(&unsub);

    assert!(!adapter.subscriptions_custom.contains(&data_type));
}

#[rstest]
fn test_instrument_subscription(
    clock: Rc<RefCell<TestClock>>,
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
    ));
    adapter.execute_subscribe(&sub);
    assert!(adapter.subscriptions_instrument.contains(&inst_id));

    // Idempotency check
    adapter.execute_subscribe(&sub);
    assert_eq!(adapter.subscriptions_instrument.len(), 1);

    let unsub = UnsubscribeCommand::Instrument(UnsubscribeInstrument::new(
        inst_id,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
    ));
    adapter.execute_unsubscribe(&unsub);
    assert!(!adapter.subscriptions_instrument.contains(&inst_id));
}

#[rstest]
fn test_instruments_subscription(
    clock: Rc<RefCell<TestClock>>,
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
    ));
    adapter.execute_subscribe(&sub);
    assert!(adapter.subscriptions_instrument_venue.contains(&venue));

    // Idempotency check
    adapter.execute_subscribe(&sub);
    assert_eq!(adapter.subscriptions_instrument_venue.len(), 1);

    let unsub = UnsubscribeCommand::Instruments(UnsubscribeInstruments::new(
        Some(client_id),
        venue,
        UUID4::new(),
        UnixNanos::default(),
        None,
    ));
    adapter.execute_unsubscribe(&unsub);
    assert!(!adapter.subscriptions_instrument_venue.contains(&venue));
}

#[rstest]
fn test_book_deltas_subscription(
    clock: Rc<RefCell<TestClock>>,
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
    ));
    adapter.execute_subscribe(&sub);
    assert!(adapter.subscriptions_book_deltas.contains(&inst_id));

    // Idempotency check
    adapter.execute_subscribe(&sub);
    assert_eq!(adapter.subscriptions_book_deltas.len(), 1);

    let unsub = UnsubscribeCommand::BookDeltas(UnsubscribeBookDeltas::new(
        inst_id,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
    ));
    adapter.execute_unsubscribe(&unsub);
    assert!(!adapter.subscriptions_book_deltas.contains(&inst_id));
}

#[rstest]
fn test_book_depth10_subscription(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let client = Box::new(MockDataClient::new(clock, cache, client_id, Some(venue)));
    let mut adapter = DataClientAdapter::new(client_id, Some(venue), false, false, client);

    let instrument = audusd_sim();
    let inst_id = instrument.id;
    let depth = NonZeroUsize::new(10);

    let sub = SubscribeCommand::BookDepth10(SubscribeBookDepth10::new(
        inst_id,
        BookType::L2_MBP,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        depth,
        false,
        None,
    ));
    adapter.execute_subscribe(&sub);
    assert!(adapter.subscriptions_book_depth10.contains(&inst_id));

    // Idempotency check
    adapter.execute_subscribe(&sub);
    assert_eq!(adapter.subscriptions_book_depth10.len(), 1);

    let unsub = UnsubscribeCommand::BookDepth10(UnsubscribeBookDepth10::new(
        inst_id,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
    ));
    adapter.execute_unsubscribe(&unsub);
    assert!(!adapter.subscriptions_book_depth10.contains(&inst_id));
}

#[rstest]
fn test_book_snapshots_subscription(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let client = Box::new(MockDataClient::new(clock, cache, client_id, Some(venue)));
    let mut adapter = DataClientAdapter::new(client_id, Some(venue), false, false, client);

    let instrument = audusd_sim();
    let inst_id = instrument.id;
    let depth = NonZeroUsize::new(10);
    let interval_ms = NonZeroUsize::new(1000).unwrap();

    let sub = SubscribeCommand::BookSnapshots(SubscribeBookSnapshots::new(
        inst_id,
        BookType::L2_MBP,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        depth,
        interval_ms,
        None,
    ));
    adapter.execute_subscribe(&sub);
    assert!(adapter.subscriptions_book_snapshots.contains(&inst_id));

    // Idempotency check
    adapter.execute_subscribe(&sub);
    assert_eq!(adapter.subscriptions_book_snapshots.len(), 1);

    let unsub = UnsubscribeCommand::BookSnapshots(UnsubscribeBookSnapshots::new(
        inst_id,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
    ));
    adapter.execute_unsubscribe(&unsub);
    assert!(!adapter.subscriptions_book_snapshots.contains(&inst_id));
}

#[rstest]
fn test_quote_subscription(
    clock: Rc<RefCell<TestClock>>,
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
    ));
    adapter.execute_subscribe(&sub);
    assert!(adapter.subscriptions_quotes.contains(&inst_id));

    // Idempotency check
    adapter.execute_subscribe(&sub);
    assert_eq!(adapter.subscriptions_quotes.len(), 1);

    let unsub = UnsubscribeCommand::Quotes(UnsubscribeQuotes::new(
        inst_id,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
    ));
    adapter.execute_unsubscribe(&unsub);
    assert!(!adapter.subscriptions_quotes.contains(&inst_id));
}

#[rstest]
fn test_trades_subscription(
    clock: Rc<RefCell<TestClock>>,
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
    ));
    adapter.execute_subscribe(&sub);
    assert!(adapter.subscriptions_trades.contains(&inst_id));

    // Idempotency check
    adapter.execute_subscribe(&sub);
    assert_eq!(adapter.subscriptions_trades.len(), 1);

    let unsub = UnsubscribeCommand::Trades(UnsubscribeTrades::new(
        inst_id,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
    ));
    adapter.execute_unsubscribe(&unsub);
    assert!(!adapter.subscriptions_trades.contains(&inst_id));
}

#[rstest]
fn test_mark_price_subscription(
    clock: Rc<RefCell<TestClock>>,
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
    ));
    adapter.execute_subscribe(&sub);
    assert!(adapter.subscriptions_mark_prices.contains(&inst_id));

    // Idempotency check
    adapter.execute_subscribe(&sub);
    assert_eq!(adapter.subscriptions_mark_prices.len(), 1);

    let unsub = UnsubscribeCommand::MarkPrices(UnsubscribeMarkPrices::new(
        inst_id,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
    ));
    adapter.execute_unsubscribe(&unsub);
    assert!(!adapter.subscriptions_mark_prices.contains(&inst_id));
}

#[rstest]
fn test_index_price_subscription(
    clock: Rc<RefCell<TestClock>>,
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
    ));
    adapter.execute_subscribe(&sub);
    assert!(adapter.subscriptions_index_prices.contains(&inst_id));

    // Idempotency check
    adapter.execute_subscribe(&sub);
    assert_eq!(adapter.subscriptions_index_prices.len(), 1);

    let unsub = UnsubscribeCommand::IndexPrices(UnsubscribeIndexPrices::new(
        inst_id,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
    ));
    adapter.execute_unsubscribe(&unsub);
    assert!(!adapter.subscriptions_index_prices.contains(&inst_id));
}

#[rstest]
fn test_funding_rate_subscription(
    clock: Rc<RefCell<TestClock>>,
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
    ));
    adapter.execute_subscribe(&sub);
    assert!(adapter.subscriptions_funding_rates.contains(&inst_id));

    // Idempotency check
    adapter.execute_subscribe(&sub);
    assert_eq!(adapter.subscriptions_funding_rates.len(), 1);

    let unsub = UnsubscribeCommand::FundingRates(UnsubscribeFundingRates::new(
        inst_id,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
    ));
    adapter.execute_unsubscribe(&unsub);
    assert!(!adapter.subscriptions_funding_rates.contains(&inst_id));
}

#[rstest]
fn test_bars_subscription(
    clock: Rc<RefCell<TestClock>>,
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
    ));
    adapter.execute_subscribe(&sub);
    assert!(adapter.subscriptions_bars.contains(&bar_type));

    // Idempotency check
    adapter.execute_subscribe(&sub);
    assert_eq!(adapter.subscriptions_bars.len(), 1);

    let unsub = UnsubscribeCommand::Bars(UnsubscribeBars::new(
        bar_type,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
    ));
    adapter.execute_unsubscribe(&unsub);
    assert!(!adapter.subscriptions_bars.contains(&bar_type));
}

#[rstest]
fn test_instrument_status_subscription(
    clock: Rc<RefCell<TestClock>>,
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
    ));
    adapter.execute_subscribe(&sub);
    assert!(adapter.subscriptions_instrument_status.contains(&inst_id));

    // Idempotency check
    adapter.execute_subscribe(&sub);
    assert_eq!(adapter.subscriptions_instrument_status.len(), 1);

    let unsub = UnsubscribeCommand::InstrumentStatus(UnsubscribeInstrumentStatus::new(
        inst_id,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
    ));
    adapter.execute_unsubscribe(&unsub);
    assert!(!adapter.subscriptions_instrument_status.contains(&inst_id));
}

#[rstest]
fn test_instrument_close_subscription(
    clock: Rc<RefCell<TestClock>>,
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
    ));
    adapter.execute_subscribe(&sub);
    assert!(adapter.subscriptions_instrument_close.contains(&inst_id));

    // Idempotency check
    adapter.execute_subscribe(&sub);
    assert_eq!(adapter.subscriptions_instrument_close.len(), 1);

    let unsub = UnsubscribeCommand::InstrumentClose(UnsubscribeInstrumentClose::new(
        inst_id,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
    ));
    adapter.execute_unsubscribe(&unsub);
    assert!(!adapter.subscriptions_instrument_close.contains(&inst_id));
}

#[rstest]
fn test_custom_data_unsubscribe_noop(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let client = Box::new(MockDataClient::new(clock, cache, client_id, Some(venue)));
    let mut adapter = DataClientAdapter::new(client_id, Some(venue), false, false, client);

    // Unsubscribe without prior subscribe should be no-op
    let data_type = DataType::new("NoOpType", None);
    let unsub = UnsubscribeCommand::Data(UnsubscribeCustomData::new(
        Some(client_id),
        Some(venue),
        data_type.clone(),
        UUID4::new(),
        UnixNanos::default(),
        None,
    ));
    adapter.execute_unsubscribe(&unsub);
    assert!(!adapter.subscriptions_custom.contains(&data_type));
    // Underlying client should not have been called (state-only test)
    assert!(adapter.subscriptions_custom.is_empty());
}

#[rstest]
fn test_custom_data_unsubscribe_idempotent(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let client = Box::new(MockDataClient::new(clock, cache, client_id, Some(venue)));
    let mut adapter = DataClientAdapter::new(client_id, Some(venue), false, false, client);

    // Subscribe then unsubscribe twice
    let data_type = DataType::new("IdemType", None);
    let sub = SubscribeCommand::Data(SubscribeCustomData::new(
        Some(client_id),
        Some(venue),
        data_type.clone(),
        UUID4::new(),
        UnixNanos::default(),
        None,
    ));
    adapter.execute_subscribe(&sub);
    let unsub = UnsubscribeCommand::Data(UnsubscribeCustomData::new(
        Some(client_id),
        Some(venue),
        data_type.clone(),
        UUID4::new(),
        UnixNanos::default(),
        None,
    ));
    adapter.execute_unsubscribe(&unsub);
    adapter.execute_unsubscribe(&unsub);
    // Expect adapter state cleared and no panic on second unsubscribe
    assert!(!adapter.subscriptions_custom.contains(&data_type));
}

#[rstest]
fn test_instrument_unsubscribe_noop(
    clock: Rc<RefCell<TestClock>>,
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
    ));
    adapter.execute_unsubscribe(&unsub);
    assert!(!adapter.subscriptions_instrument.contains(&inst_id));
    // Underlying client should not have been called
    assert!(adapter.subscriptions_instrument.is_empty());
}

#[rstest]
fn test_instrument_unsubscribe_idempotent(
    clock: Rc<RefCell<TestClock>>,
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
    ));
    adapter.execute_subscribe(&sub);
    let unsub = UnsubscribeCommand::Instrument(UnsubscribeInstrument::new(
        inst_id,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
    ));
    adapter.execute_unsubscribe(&unsub);
    adapter.execute_unsubscribe(&unsub);
    // Expect adapter state cleared and no panic on second unsubscribe
    assert!(!adapter.subscriptions_instrument.contains(&inst_id));
}

#[rstest]
fn test_instruments_unsubscribe_noop(
    clock: Rc<RefCell<TestClock>>,
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
    ));
    adapter.execute_unsubscribe(&unsub);
    assert!(adapter.subscriptions_instrument_venue.is_empty());
}

#[rstest]
fn test_instruments_unsubscribe_idempotent(
    clock: Rc<RefCell<TestClock>>,
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
    ));
    adapter.execute_subscribe(&sub);

    let unsub = UnsubscribeCommand::Instruments(UnsubscribeInstruments::new(
        Some(client_id),
        venue,
        UUID4::new(),
        UnixNanos::default(),
        None,
    ));

    adapter.execute_unsubscribe(&unsub);
    adapter.execute_unsubscribe(&unsub);
    assert!(adapter.subscriptions_instrument_venue.is_empty());
}
#[rstest]
fn test_book_deltas_unsubscribe_noop(
    clock: Rc<RefCell<TestClock>>,
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
    ));
    adapter.execute_unsubscribe(&unsub);
    assert!(adapter.subscriptions_book_deltas.is_empty());
}

#[rstest]
fn test_book_deltas_unsubscribe_idempotent(
    clock: Rc<RefCell<TestClock>>,
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
    ));
    adapter.execute_subscribe(&sub);

    let unsub = UnsubscribeCommand::BookDeltas(UnsubscribeBookDeltas::new(
        inst_id,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
    ));

    adapter.execute_unsubscribe(&unsub);
    adapter.execute_unsubscribe(&unsub);
    assert!(adapter.subscriptions_book_deltas.is_empty());
}

#[rstest]
fn test_book_depth10_unsubscribe_noop(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let client = Box::new(MockDataClient::new(clock, cache, client_id, Some(venue)));
    let mut adapter = DataClientAdapter::new(client_id, Some(venue), false, false, client);
    let inst_id = audusd_sim().id;
    let unsub = UnsubscribeCommand::BookDepth10(UnsubscribeBookDepth10::new(
        inst_id,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
    ));
    adapter.execute_unsubscribe(&unsub);
    assert!(adapter.subscriptions_book_depth10.is_empty());
}

#[rstest]
fn test_book_depth10_unsubscribe_idempotent(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let client = Box::new(MockDataClient::new(clock, cache, client_id, Some(venue)));
    let mut adapter = DataClientAdapter::new(client_id, Some(venue), false, false, client);
    let inst_id = audusd_sim().id;
    let sub = SubscribeCommand::BookDepth10(SubscribeBookDepth10::new(
        inst_id,
        BookType::L2_MBP,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        NonZeroUsize::new(10),
        false,
        None,
    ));
    adapter.execute_subscribe(&sub);
    let unsub = UnsubscribeCommand::BookDepth10(UnsubscribeBookDepth10::new(
        inst_id,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
    ));
    adapter.execute_unsubscribe(&unsub);
    adapter.execute_unsubscribe(&unsub);
    assert!(adapter.subscriptions_book_depth10.is_empty());
}

#[rstest]
fn test_book_snapshots_unsubscribe_noop(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let client = Box::new(MockDataClient::new(clock, cache, client_id, Some(venue)));
    let mut adapter = DataClientAdapter::new(client_id, Some(venue), false, false, client);
    let inst_id = audusd_sim().id;
    let unsub = UnsubscribeCommand::BookSnapshots(UnsubscribeBookSnapshots::new(
        inst_id,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
    ));
    adapter.execute_unsubscribe(&unsub);
    assert!(adapter.subscriptions_book_snapshots.is_empty());
}

#[rstest]
fn test_book_snapshots_unsubscribe_idempotent(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let client = Box::new(MockDataClient::new(clock, cache, client_id, Some(venue)));
    let mut adapter = DataClientAdapter::new(client_id, Some(venue), false, false, client);
    let inst_id = audusd_sim().id;
    let sub = SubscribeCommand::BookSnapshots(SubscribeBookSnapshots::new(
        inst_id,
        BookType::L2_MBP,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        Some(NonZeroUsize::new(10).unwrap()),
        NonZeroUsize::new(1000).unwrap(),
        None,
    ));
    adapter.execute_subscribe(&sub);
    let unsub = UnsubscribeCommand::BookSnapshots(UnsubscribeBookSnapshots::new(
        inst_id,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
    ));
    adapter.execute_unsubscribe(&unsub);
    adapter.execute_unsubscribe(&unsub);
    assert!(adapter.subscriptions_book_snapshots.is_empty());
}

#[rstest]
fn test_quotes_unsubscribe_noop(
    clock: Rc<RefCell<TestClock>>,
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
    ));
    adapter.execute_unsubscribe(&unsub);
    assert!(adapter.subscriptions_quotes.is_empty());
}

#[rstest]
fn test_quotes_unsubscribe_idempotent(
    clock: Rc<RefCell<TestClock>>,
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
    ));
    adapter.execute_subscribe(&sub);
    let unsub = UnsubscribeCommand::Quotes(UnsubscribeQuotes::new(
        inst_id,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
    ));
    adapter.execute_unsubscribe(&unsub);
    adapter.execute_unsubscribe(&unsub);
    assert!(adapter.subscriptions_quotes.is_empty());
}

#[rstest]
fn test_trades_unsubscribe_noop(
    clock: Rc<RefCell<TestClock>>,
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
    ));
    adapter.execute_unsubscribe(&unsub);
    assert!(adapter.subscriptions_trades.is_empty());
}

#[rstest]
fn test_trades_unsubscribe_idempotent(
    clock: Rc<RefCell<TestClock>>,
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
    ));
    adapter.execute_subscribe(&sub);
    let unsub = UnsubscribeCommand::Trades(UnsubscribeTrades::new(
        inst_id,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
    ));
    adapter.execute_unsubscribe(&unsub);
    adapter.execute_unsubscribe(&unsub);
    assert!(adapter.subscriptions_trades.is_empty());
}

#[rstest]
fn test_bars_unsubscribe_noop(
    clock: Rc<RefCell<TestClock>>,
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
    ));
    adapter.execute_unsubscribe(&unsub);
    assert!(adapter.subscriptions_bars.is_empty());
}

#[rstest]
fn test_bars_unsubscribe_idempotent(
    clock: Rc<RefCell<TestClock>>,
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
    ));
    adapter.execute_subscribe(&sub);
    let unsub = UnsubscribeCommand::Bars(UnsubscribeBars::new(
        bar_type,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
    ));
    adapter.execute_unsubscribe(&unsub);
    adapter.execute_unsubscribe(&unsub);
    assert!(adapter.subscriptions_bars.is_empty());
}

#[rstest]
fn test_mark_prices_unsubscribe_noop(
    clock: Rc<RefCell<TestClock>>,
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
    ));
    adapter.execute_unsubscribe(&unsub);
    assert!(adapter.subscriptions_mark_prices.is_empty());
}

#[rstest]
fn test_mark_prices_unsubscribe_idempotent(
    clock: Rc<RefCell<TestClock>>,
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
    ));
    adapter.execute_subscribe(&sub);
    let unsub = UnsubscribeCommand::MarkPrices(UnsubscribeMarkPrices::new(
        inst_id,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
    ));
    adapter.execute_unsubscribe(&unsub);
    adapter.execute_unsubscribe(&unsub);
    assert!(adapter.subscriptions_mark_prices.is_empty());
}

#[rstest]
fn test_index_prices_unsubscribe_noop(
    clock: Rc<RefCell<TestClock>>,
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
    ));
    adapter.execute_unsubscribe(&unsub);
    assert!(adapter.subscriptions_index_prices.is_empty());
}

#[rstest]
fn test_index_prices_unsubscribe_idempotent(
    clock: Rc<RefCell<TestClock>>,
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
    ));
    adapter.execute_subscribe(&sub);
    let unsub = UnsubscribeCommand::IndexPrices(UnsubscribeIndexPrices::new(
        inst_id,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
    ));
    adapter.execute_unsubscribe(&unsub);
    adapter.execute_unsubscribe(&unsub);
    assert!(adapter.subscriptions_index_prices.is_empty());
}

#[rstest]
fn test_funding_rates_unsubscribe_noop(
    clock: Rc<RefCell<TestClock>>,
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
    ));
    adapter.execute_unsubscribe(&unsub);
    assert!(!adapter.subscriptions_funding_rates.contains(&inst_id));
}

#[rstest]
fn test_funding_rates_unsubscribe_idempotent(
    clock: Rc<RefCell<TestClock>>,
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
    ));
    adapter.execute_subscribe(&sub);
    assert!(adapter.subscriptions_funding_rates.contains(&inst_id));

    let unsub = UnsubscribeCommand::FundingRates(UnsubscribeFundingRates::new(
        inst_id,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
    ));
    adapter.execute_unsubscribe(&unsub);
    assert!(!adapter.subscriptions_funding_rates.contains(&inst_id));

    adapter.execute_unsubscribe(&unsub);
    assert!(!adapter.subscriptions_funding_rates.contains(&inst_id));
}

#[rstest]
fn test_instrument_status_unsubscribe_noop(
    clock: Rc<RefCell<TestClock>>,
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
    ));
    adapter.execute_unsubscribe(&unsub);
    assert!(adapter.subscriptions_instrument_status.is_empty());
}

#[rstest]
fn test_instrument_status_unsubscribe_idempotent(
    clock: Rc<RefCell<TestClock>>,
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
    ));
    adapter.execute_subscribe(&sub);
    let unsub = UnsubscribeCommand::InstrumentStatus(UnsubscribeInstrumentStatus::new(
        inst_id,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
    ));
    adapter.execute_unsubscribe(&unsub);
    adapter.execute_unsubscribe(&unsub);
    assert!(adapter.subscriptions_instrument_status.is_empty());
}

#[rstest]
fn test_instrument_close_unsubscribe_noop(
    clock: Rc<RefCell<TestClock>>,
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
    ));
    adapter.execute_unsubscribe(&unsub);
    assert!(adapter.subscriptions_instrument_close.is_empty());
}

#[rstest]
fn test_instrument_close_unsubscribe_idempotent(
    clock: Rc<RefCell<TestClock>>,
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
    ));
    adapter.execute_subscribe(&sub);

    let unsub = UnsubscribeCommand::InstrumentClose(UnsubscribeInstrumentClose::new(
        inst_id,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
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
    clock: Rc<RefCell<TestClock>>,
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
        Some(recorder.clone()),
    ));
    let adapter = DataClientAdapter::new(client_id, Some(venue), false, false, client);

    let data_type = DataType::new("ReqType", None);
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
    adapter.request_data(&req).unwrap();

    let rec = recorder.borrow();
    assert_eq!(rec.len(), 1);
    assert_eq!(rec[0], DataCommand::Request(RequestCommand::Data(req)));
}

#[rstest]
fn test_request_instrument(
    clock: Rc<RefCell<TestClock>>,
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
        Some(recorder.clone()),
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
    adapter.request_instrument(&req).unwrap();

    let rec = recorder.borrow();
    assert_eq!(rec.len(), 1);
    assert_eq!(
        rec[0],
        DataCommand::Request(RequestCommand::Instrument(req))
    );
}

#[rstest]
fn test_request_instruments(
    clock: Rc<RefCell<TestClock>>,
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
        Some(recorder.clone()),
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
    adapter.request_instruments(&req).unwrap();

    let rec = recorder.borrow();
    assert_eq!(rec.len(), 1);
    assert_eq!(
        rec[0],
        DataCommand::Request(RequestCommand::Instruments(req))
    );
}

#[rstest]
fn test_request_book_snapshot(
    clock: Rc<RefCell<TestClock>>,
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
        Some(recorder.clone()),
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
    adapter.request_book_snapshot(&req).unwrap();

    let rec = recorder.borrow();
    assert_eq!(rec.len(), 1);
    assert_eq!(
        rec[0],
        DataCommand::Request(RequestCommand::BookSnapshot(req))
    );
}

#[rstest]
fn test_request_quotes(
    clock: Rc<RefCell<TestClock>>,
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
        Some(recorder.clone()),
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
    adapter.request_quotes(&req).unwrap();

    let rec = recorder.borrow();
    assert_eq!(rec.len(), 1);
    assert_eq!(rec[0], DataCommand::Request(RequestCommand::Quotes(req)));
}

#[rstest]
fn test_request_trades(
    clock: Rc<RefCell<TestClock>>,
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
        Some(recorder.clone()),
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
    adapter.request_trades(&req).unwrap();

    let rec = recorder.borrow();
    assert_eq!(rec.len(), 1);
    assert_eq!(rec[0], DataCommand::Request(RequestCommand::Trades(req)));
}

#[rstest]
fn test_request_bars(
    clock: Rc<RefCell<TestClock>>,
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
        Some(recorder.clone()),
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
    adapter.request_bars(&req).unwrap();

    let rec = recorder.borrow();
    assert_eq!(rec.len(), 1);
    assert_eq!(rec[0], DataCommand::Request(RequestCommand::Bars(req)));
}

#[rstest]
fn test_request_order_book_depth(
    clock: Rc<RefCell<TestClock>>,
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
        Some(recorder.clone()),
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
    adapter.request_book_depth(&req).unwrap();

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
    clock: Rc<RefCell<TestClock>>,
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
    adapter.execute_defi_subscribe(&sub);
    assert!(adapter.subscriptions_blocks.contains(&blockchain));

    // Idempotency check
    adapter.execute_defi_subscribe(&sub);
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
fn test_defi_pool_swaps_subscription(
    clock: Rc<RefCell<TestClock>>,
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
    adapter.execute_defi_subscribe(&sub);
    assert!(adapter.subscriptions_pool_swaps.contains(&instrument_id));

    // Idempotency check
    adapter.execute_defi_subscribe(&sub);
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
    clock: Rc<RefCell<TestClock>>,
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
    clock: Rc<RefCell<TestClock>>,
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
    adapter.execute_defi_subscribe(&sub);

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
    clock: Rc<RefCell<TestClock>>,
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
    clock: Rc<RefCell<TestClock>>,
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
    adapter.execute_defi_subscribe(&sub);

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
