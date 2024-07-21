// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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

//! Base data client functionality.

// Under development
#![allow(dead_code)]
#![allow(unused_variables)]

use std::{any::Any, cell::RefCell, collections::HashSet, rc::Rc, sync::Arc};

use indexmap::IndexMap;
use nautilus_common::{
    cache::Cache, clock::Clock, messages::data::DataResponse, msgbus::MessageBus,
};
use nautilus_core::{correctness, nanos::UnixNanos, uuid::UUID4};
use nautilus_model::{
    data::{
        bar::{Bar, BarType},
        quote::QuoteTick,
        trade::TradeTick,
        Data, DataType,
    },
    enums::BookType,
    identifiers::{ClientId, InstrumentId, Venue},
    instruments::any::InstrumentAny,
};

pub trait DataClient {
    fn client_id(&self) -> ClientId;
    fn venue(&self) -> Option<Venue>;
    fn start(&self);
    fn stop(&self);
    fn reset(&self);
    fn dispose(&self);
    fn is_connected(&self) -> bool;
    fn is_disconnected(&self) -> bool;
    fn subscribed_generic_data(&self) -> &HashSet<DataType>;
    fn subscribed_instrument_venues(&self) -> &HashSet<Venue>;
    fn subscribed_instruments(&self) -> &HashSet<InstrumentId>;
    fn subscribed_order_book_deltas(&self) -> &HashSet<InstrumentId>;
    fn subscribed_order_book_snapshots(&self) -> &HashSet<InstrumentId>;
    fn subscribed_quote_ticks(&self) -> &HashSet<InstrumentId>;
    fn subscribed_trade_ticks(&self) -> &HashSet<InstrumentId>;
    fn subscribed_bars(&self) -> &HashSet<BarType>;
    fn subscribed_instrument_status(&self) -> &HashSet<InstrumentId>;
    fn subscribed_instrument_close(&self) -> &HashSet<InstrumentId>;
    fn subscribe(&mut self, data_type: DataType) -> anyhow::Result<()>;
    fn subscribe_instruments(&mut self, venue: Option<Venue>) -> anyhow::Result<()>;
    fn subscribe_instrument(&mut self, instrument_id: InstrumentId) -> anyhow::Result<()>;
    fn subscribe_order_book_deltas(
        &mut self,
        instrument_id: InstrumentId,
        book_type: BookType,
        depth: Option<usize>,
    ) -> anyhow::Result<()>;
    fn subscribe_order_book_snapshots(
        &mut self,
        instrument_id: InstrumentId,
        book_type: BookType,
        depth: Option<usize>,
    ) -> anyhow::Result<()>;
    fn subscribe_quote_ticks(&mut self, instrument_id: InstrumentId) -> anyhow::Result<()>;
    fn subscribe_trade_ticks(&mut self, instrument_id: InstrumentId) -> anyhow::Result<()>;
    fn subscribe_bars(&mut self, bar_type: BarType) -> anyhow::Result<()>;
    fn subscribe_instrument_status(&mut self, instrument_id: InstrumentId) -> anyhow::Result<()>;
    fn subscribe_instrument_close(&mut self, instrument_id: InstrumentId) -> anyhow::Result<()>;
    fn unsubscribe(&mut self, data_type: DataType) -> anyhow::Result<()>;
    fn unsubscribe_instruments(&mut self, venue: Option<Venue>) -> anyhow::Result<()>;
    fn unsubscribe_instrument(&mut self, instrument_id: InstrumentId) -> anyhow::Result<()>;
    fn unsubscribe_order_book_deltas(&mut self, instrument_id: InstrumentId) -> anyhow::Result<()>;
    fn unsubscribe_order_book_snapshots(
        &mut self,
        instrument_id: InstrumentId,
    ) -> anyhow::Result<()>;
    fn unsubscribe_quote_ticks(&mut self, instrument_id: InstrumentId) -> anyhow::Result<()>;
    fn unsubscribe_trade_ticks(&mut self, instrument_id: InstrumentId) -> anyhow::Result<()>;
    fn unsubscribe_bars(&mut self, bar_type: BarType) -> anyhow::Result<()>;
    fn unsubscribe_instrument_status(&mut self, instrument_id: InstrumentId) -> anyhow::Result<()>;
    fn unsubscribe_instrument_close(&mut self, instrument_id: InstrumentId) -> anyhow::Result<()>;
    fn request(&mut self, correlation_id: UUID4, data_type: DataType);
    fn request_instruments(
        &mut self,
        correlation_id: UUID4,
        venue: Venue,
        start: Option<UnixNanos>,
        end: Option<UnixNanos>,
    );
    fn request_instrument(
        &mut self,
        correlation_id: UUID4,
        instrument_id: InstrumentId,
        start: Option<UnixNanos>,
        end: Option<UnixNanos>,
    );
    fn request_order_book_snapshot(
        &mut self,
        correlation_id: UUID4,
        instrument_id: InstrumentId,
        depth: Option<usize>,
    );
    fn request_quote_ticks(
        &mut self,
        correlation_id: UUID4,
        instrument_id: InstrumentId,
        start: Option<UnixNanos>,
        end: Option<UnixNanos>,
        limit: Option<usize>,
    );
    fn request_trade_ticks(
        &mut self,
        correlation_id: UUID4,
        instrument_id: InstrumentId,
        start: Option<UnixNanos>,
        end: Option<UnixNanos>,
        limit: Option<usize>,
    );
    fn request_bars(
        &mut self,
        correlation_id: UUID4,
        bar_type: BarType,
        start: Option<UnixNanos>,
        end: Option<UnixNanos>,
        limit: Option<usize>,
    );
}

pub struct DataClientCore {
    pub client_id: ClientId,
    pub venue: Venue,
    pub is_connected: bool,
    clock: Box<dyn Clock>,
    cache: Rc<RefCell<Cache>>,
    msgbus: Rc<RefCell<MessageBus>>,
    subscriptions_generic: HashSet<DataType>,
    subscriptions_order_book_delta: HashSet<InstrumentId>,
    subscriptions_order_book_snapshot: HashSet<InstrumentId>,
    subscriptions_quote_tick: HashSet<InstrumentId>,
    subscriptions_trade_tick: HashSet<InstrumentId>,
    subscriptions_bar: HashSet<BarType>,
    subscriptions_instrument_status: HashSet<InstrumentId>,
    subscriptions_instrument_close: HashSet<InstrumentId>,
    subscriptions_instrument: HashSet<InstrumentId>,
    subscriptions_instrument_venue: HashSet<Venue>,
}

impl DataClientCore {
    #[must_use]
    pub const fn subscribed_generic(&self) -> &HashSet<DataType> {
        &self.subscriptions_generic
    }

    #[must_use]
    pub const fn subscribed_instrument_venues(&self) -> &HashSet<Venue> {
        &self.subscriptions_instrument_venue
    }

    #[must_use]
    pub const fn subscribed_instruments(&self) -> &HashSet<InstrumentId> {
        &self.subscriptions_instrument
    }

    #[must_use]
    pub const fn subscribed_order_book_deltas(&self) -> &HashSet<InstrumentId> {
        &self.subscriptions_order_book_delta
    }

    #[must_use]
    pub const fn subscribed_order_book_snapshots(&self) -> &HashSet<InstrumentId> {
        &self.subscriptions_order_book_snapshot
    }

    #[must_use]
    pub const fn subscribed_quote_ticks(&self) -> &HashSet<InstrumentId> {
        &self.subscriptions_quote_tick
    }

    #[must_use]
    pub const fn subscribed_trade_ticks(&self) -> &HashSet<InstrumentId> {
        &self.subscriptions_trade_tick
    }

    #[must_use]
    pub const fn subscribed_bars(&self) -> &HashSet<BarType> {
        &self.subscriptions_bar
    }

    #[must_use]
    pub const fn subscribed_instrument_status(&self) -> &HashSet<InstrumentId> {
        &self.subscriptions_instrument_status
    }

    #[must_use]
    pub const fn subscribed_instrument_close(&self) -> &HashSet<InstrumentId> {
        &self.subscriptions_instrument_close
    }

    pub fn add_subscription_generic(&mut self, data_type: DataType) -> anyhow::Result<()> {
        correctness::check_member_not_in_set(
            &data_type,
            &self.subscriptions_generic,
            "data_type",
            "subscriptions_generic",
        )?;
        self.subscriptions_generic.insert(data_type);
        Ok(())
    }

    pub fn add_subscription_instrument_venue(&mut self, venue: Venue) -> anyhow::Result<()> {
        correctness::check_member_not_in_set(
            &venue,
            &self.subscriptions_instrument_venue,
            "venue",
            "subscriptions_instrument_venue",
        )?;
        self.subscriptions_instrument_venue.insert(venue);
        Ok(())
    }

    pub fn add_subscription_instrument(
        &mut self,
        instrument_id: InstrumentId,
    ) -> anyhow::Result<()> {
        correctness::check_member_not_in_set(
            &instrument_id,
            &self.subscriptions_instrument,
            "instrument_id",
            "subscriptions_instrument",
        )?;
        self.subscriptions_instrument.insert(instrument_id);
        Ok(())
    }

    pub fn add_subscription_order_book_deltas(
        &mut self,
        instrument_id: InstrumentId,
    ) -> anyhow::Result<()> {
        correctness::check_member_not_in_set(
            &instrument_id,
            &self.subscriptions_order_book_delta,
            "instrument_id",
            "subscriptions_order_book_delta",
        )?;
        self.subscriptions_order_book_delta.insert(instrument_id);
        Ok(())
    }

    pub fn add_subscription_order_book_snapshots(
        &mut self,
        instrument_id: InstrumentId,
    ) -> anyhow::Result<()> {
        correctness::check_member_not_in_set(
            &instrument_id,
            &self.subscriptions_order_book_snapshot,
            "instrument_id",
            "subscriptions_order_book_snapshot",
        )?;
        self.subscriptions_order_book_snapshot.insert(instrument_id);
        Ok(())
    }

    pub fn add_subscription_quote_ticks(
        &mut self,
        instrument_id: InstrumentId,
    ) -> anyhow::Result<()> {
        correctness::check_member_not_in_set(
            &instrument_id,
            &self.subscriptions_quote_tick,
            "instrument_id",
            "subscriptions_quote_tick",
        )?;
        self.subscriptions_quote_tick.insert(instrument_id);
        Ok(())
    }

    pub fn add_subscription_trade_ticks(
        &mut self,
        instrument_id: InstrumentId,
    ) -> anyhow::Result<()> {
        correctness::check_member_not_in_set(
            &instrument_id,
            &self.subscriptions_trade_tick,
            "instrument_id",
            "subscriptions_trade_tick",
        )?;
        self.subscriptions_trade_tick.insert(instrument_id);
        Ok(())
    }

    pub fn add_subscription_bars(&mut self, bar_type: BarType) -> anyhow::Result<()> {
        correctness::check_member_not_in_set(
            &bar_type,
            &self.subscriptions_bar,
            "bar_type",
            "subscriptions_bar",
        )?;
        self.subscriptions_bar.insert(bar_type);
        Ok(())
    }

    pub fn add_subscription_instrument_status(
        &mut self,
        instrument_id: InstrumentId,
    ) -> anyhow::Result<()> {
        correctness::check_member_not_in_set(
            &instrument_id,
            &self.subscriptions_instrument_status,
            "instrument_id",
            "subscriptions_instrument_status",
        )?;
        self.subscriptions_instrument_status.insert(instrument_id);
        Ok(())
    }

    pub fn add_subscription_instrument_close(
        &mut self,
        instrument_id: InstrumentId,
    ) -> anyhow::Result<()> {
        correctness::check_member_not_in_set(
            &instrument_id,
            &self.subscriptions_instrument_close,
            "instrument_id",
            "subscriptions_instrument_close",
        )?;
        self.subscriptions_instrument_close.insert(instrument_id);
        Ok(())
    }

    pub fn remove_subscription_generic(&mut self, data_type: &DataType) -> anyhow::Result<()> {
        correctness::check_member_in_set(
            data_type,
            &self.subscriptions_generic,
            "data_type",
            "subscriptions_generic",
        )?;
        self.subscriptions_generic.remove(data_type);
        Ok(())
    }

    pub fn remove_subscription_instrument_venue(&mut self, venue: &Venue) -> anyhow::Result<()> {
        correctness::check_member_in_set(
            venue,
            &self.subscriptions_instrument_venue,
            "venue",
            "subscriptions_instrument_venue",
        )?;
        self.subscriptions_instrument_venue.remove(venue);
        Ok(())
    }

    pub fn remove_subscription_instrument(
        &mut self,
        instrument_id: &InstrumentId,
    ) -> anyhow::Result<()> {
        correctness::check_member_in_set(
            instrument_id,
            &self.subscriptions_instrument,
            "instrument_id",
            "subscriptions_instrument",
        )?;
        self.subscriptions_instrument.remove(instrument_id);
        Ok(())
    }

    pub fn remove_subscription_order_book_delta(
        &mut self,
        instrument_id: &InstrumentId,
    ) -> anyhow::Result<()> {
        correctness::check_member_in_set(
            instrument_id,
            &self.subscriptions_order_book_delta,
            "instrument_id",
            "subscriptions_order_book_delta",
        )?;
        self.subscriptions_order_book_delta.remove(instrument_id);
        Ok(())
    }

    pub fn remove_subscription_order_book_snapshots(
        &mut self,
        instrument_id: &InstrumentId,
    ) -> anyhow::Result<()> {
        correctness::check_member_in_set(
            instrument_id,
            &self.subscriptions_order_book_snapshot,
            "instrument_id",
            "subscriptions_order_book_snapshot",
        )?;
        self.subscriptions_order_book_snapshot.remove(instrument_id);
        Ok(())
    }

    pub fn remove_subscription_quote_ticks(
        &mut self,
        instrument_id: &InstrumentId,
    ) -> anyhow::Result<()> {
        correctness::check_member_in_set(
            instrument_id,
            &self.subscriptions_quote_tick,
            "instrument_id",
            "subscriptions_quote_tick",
        )?;
        self.subscriptions_quote_tick.remove(instrument_id);
        Ok(())
    }

    pub fn remove_subscription_trade_ticks(
        &mut self,
        instrument_id: &InstrumentId,
    ) -> anyhow::Result<()> {
        correctness::check_member_in_set(
            instrument_id,
            &self.subscriptions_trade_tick,
            "instrument_id",
            "subscriptions_trade_tick",
        )?;
        self.subscriptions_trade_tick.remove(instrument_id);
        Ok(())
    }

    pub fn remove_subscription_bars(&mut self, bar_type: &BarType) -> anyhow::Result<()> {
        correctness::check_member_in_set(
            bar_type,
            &self.subscriptions_bar,
            "bar_type",
            "subscriptions_bar",
        )?;
        self.subscriptions_bar.remove(bar_type);
        Ok(())
    }

    pub fn remove_subscription_instrument_status(
        &mut self,
        instrument_id: &InstrumentId,
    ) -> anyhow::Result<()> {
        correctness::check_member_in_set(
            instrument_id,
            &self.subscriptions_instrument_status,
            "instrument_id",
            "subscriptions_instrument_status",
        )?;
        self.subscriptions_instrument_status.remove(instrument_id);
        Ok(())
    }

    pub fn remove_subscription_instrument_close(
        &mut self,
        instrument_id: &InstrumentId,
    ) -> anyhow::Result<()> {
        correctness::check_member_in_set(
            instrument_id,
            &self.subscriptions_instrument_close,
            "instrument_id",
            "subscriptions_instrument_close",
        )?;
        self.subscriptions_instrument_close.remove(instrument_id);
        Ok(())
    }

    pub fn handle_data(&self, data: Data) {
        self.msgbus
            .borrow()
            .send("DataEngine.process", &data as &dyn Any); // TODO: Optimize
    }

    pub fn handle_instrument(&self, instrument: InstrumentAny, correlation_id: UUID4) {
        let instrument_id = instrument.id();
        let metadata = IndexMap::from([("instrument_id".to_string(), instrument_id.to_string())]);
        let data_type = DataType::new(stringify!(InstrumentAny), Some(metadata));
        let data = Arc::new(instrument);

        let response = DataResponse::new(
            UUID4::new(),
            correlation_id,
            self.client_id,
            instrument_id.venue,
            data_type,
            data,
            self.clock.timestamp_ns(),
        );

        self.msgbus
            .borrow()
            .send("DataEngine.response", &response as &dyn Any); // TODO: Optimize
    }

    pub fn handle_instruments(
        &self,
        venue: Venue,
        instruments: Vec<InstrumentAny>,
        correlation_id: UUID4,
    ) {
        let metadata = IndexMap::from([("venue".to_string(), venue.to_string())]);
        let data_type = DataType::new(stringify!(InstrumentAny), Some(metadata));
        let data = Arc::new(instruments);

        let response = DataResponse::new(
            UUID4::new(),
            correlation_id,
            self.client_id,
            venue,
            data_type,
            data,
            self.clock.timestamp_ns(),
        );

        self.msgbus
            .borrow()
            .send("DataEngine.response", &response as &dyn Any); // TODO: Optimize
    }

    pub fn handle_quote_ticks(
        &self,
        instrument_id: &InstrumentId,
        quotes: Vec<QuoteTick>,
        correlation_id: UUID4,
    ) {
        let metadata = IndexMap::from([("instrument_id".to_string(), instrument_id.to_string())]);
        let data_type = DataType::new(stringify!(QuoteTick), Some(metadata));
        let data = Arc::new(quotes);

        let response = DataResponse::new(
            UUID4::new(),
            correlation_id,
            self.client_id,
            instrument_id.venue,
            data_type,
            data,
            self.clock.timestamp_ns(),
        );

        self.msgbus
            .borrow()
            .send("DataEngine.response", &response as &dyn Any); // TODO: Optimize
    }

    pub fn handle_trade_ticks(
        &self,
        instrument_id: &InstrumentId,
        trades: Vec<TradeTick>,
        correlation_id: UUID4,
    ) {
        let metadata = IndexMap::from([("instrument_id".to_string(), instrument_id.to_string())]);
        let data_type = DataType::new(stringify!(TradeTick), Some(metadata));
        let data = Arc::new(trades);

        let response = DataResponse::new(
            UUID4::new(),
            correlation_id,
            self.client_id,
            instrument_id.venue,
            data_type,
            data,
            self.clock.timestamp_ns(),
        );

        self.msgbus
            .borrow()
            .send("DataEngine.response", &response as &dyn Any); // TODO: Optimize
    }

    pub fn handle_bars(&self, bar_type: &BarType, bars: Vec<Bar>, correlation_id: UUID4) {
        let metadata = IndexMap::from([("bar_type".to_string(), bar_type.to_string())]);
        let data_type = DataType::new(stringify!(Bar), Some(metadata));
        let data = Arc::new(bars);

        let response = DataResponse::new(
            UUID4::new(),
            correlation_id,
            self.client_id,
            bar_type.instrument_id.venue,
            data_type,
            data,
            self.clock.timestamp_ns(),
        );

        self.msgbus
            .borrow()
            .send("DataEngine.response", &response as &dyn Any); // TODO: Optimize
    }
}
