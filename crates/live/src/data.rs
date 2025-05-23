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

use std::{cell::Ref, fmt::Display};

use nautilus_common::{
    clock::Clock,
    messages::{
        DataEvent,
        data::{
            BarsResponse, BookResponse, DataResponse, InstrumentResponse, InstrumentsResponse,
            QuotesResponse, TradesResponse,
        },
    },
};
use nautilus_core::UUID4;
use nautilus_data::client::DataClient;
use nautilus_model::{
    data::{
        Bar, BarType, Data, IndexPriceUpdate, MarkPriceUpdate, OrderBookDelta, OrderBookDeltas_API,
        OrderBookDepth10, QuoteTick, TradeTick, close::InstrumentClose,
    },
    identifiers::{ClientId, InstrumentId, Venue},
    instruments::{Instrument, InstrumentAny},
    orderbook::OrderBook,
};

#[async_trait::async_trait]
pub trait LiveDataClient: DataClient {
    fn get_message_channel(&self) -> tokio::sync::mpsc::UnboundedSender<DataEvent>;

    fn get_clock(&self) -> Ref<'_, dyn Clock>;

    fn send_delta(&self, delta: OrderBookDelta) {
        self.send_data(Data::Delta(delta));
    }

    fn send_deltas(&self, deltas: OrderBookDeltas_API) {
        self.send_data(Data::Deltas(deltas));
    }

    fn send_depth10(&self, depth: OrderBookDepth10) {
        self.send_data(Data::Depth10(Box::new(depth)));
    }

    fn send_quote(&self, quote: QuoteTick) {
        self.send_data(Data::Quote(quote));
    }

    fn send_trade(&self, trade: TradeTick) {
        self.send_data(Data::Trade(trade));
    }

    fn send_bar(&self, bar: Bar) {
        self.send_data(Data::Bar(bar));
    }

    fn send_mark_price(&self, mark_price: MarkPriceUpdate) {
        self.send_data(Data::MarkPriceUpdate(mark_price));
    }

    fn send_index_price(&self, index_price: IndexPriceUpdate) {
        self.send_data(Data::IndexPriceUpdate(index_price));
    }

    fn send_instrument_close(&self, close: InstrumentClose) {
        self.send_data(Data::InstrumentClose(close));
    }

    fn send_data(&self, data: Data) {
        if let Err(e) = self.get_message_channel().send(DataEvent::Data(data)) {
            log_send_error(&self.client_id(), &e);
        }
    }

    fn send_instrument_response(&self, instrument: InstrumentAny, correlation_id: UUID4) {
        let response = DataResponse::Instrument(Box::new(InstrumentResponse::new(
            correlation_id,
            self.client_id(),
            instrument.id(),
            instrument,
            self.get_clock().timestamp_ns(),
            None,
        )));

        self.send_response(response);
    }

    fn send_instruments_response(
        &self,
        venue: Venue,
        instruments: Vec<InstrumentAny>,
        correlation_id: UUID4,
    ) {
        let response = DataResponse::Instruments(InstrumentsResponse::new(
            correlation_id,
            self.client_id(),
            venue,
            instruments,
            self.get_clock().timestamp_ns(),
            None,
        ));

        self.send_response(response);
    }

    fn send_book_response(&self, book: OrderBook, correlation_id: UUID4) {
        let response = DataResponse::Book(BookResponse::new(
            correlation_id,
            self.client_id(),
            book.instrument_id,
            book,
            self.get_clock().timestamp_ns(),
            None,
        ));

        self.send_response(response);
    }

    fn send_quotes_response(
        &self,
        instrument_id: InstrumentId,
        quotes: Vec<QuoteTick>,
        correlation_id: UUID4,
    ) {
        let response = DataResponse::Quotes(QuotesResponse::new(
            correlation_id,
            self.client_id(),
            instrument_id,
            quotes,
            self.get_clock().timestamp_ns(),
            None,
        ));

        self.send_response(response);
    }

    fn send_trades_response(
        &self,
        instrument_id: InstrumentId,
        trades: Vec<TradeTick>,
        correlation_id: UUID4,
    ) {
        let response = DataResponse::Trades(TradesResponse::new(
            correlation_id,
            self.client_id(),
            instrument_id,
            trades,
            self.get_clock().timestamp_ns(),
            None,
        ));

        self.send_response(response);
    }

    fn send_bars(&self, bar_type: BarType, bars: Vec<Bar>, correlation_id: UUID4) {
        let response = DataResponse::Bars(BarsResponse::new(
            correlation_id,
            self.client_id(),
            bar_type,
            bars,
            self.get_clock().timestamp_ns(),
            None,
        ));

        self.send_response(response);
    }

    fn send_response(&self, response: DataResponse) {
        if let Err(e) = self
            .get_message_channel()
            .send(DataEvent::Response(response))
        {
            log_send_error(&self.client_id(), &e);
        }
    }
}

#[inline(always)]
fn log_send_error<E: Display>(client_id: &ClientId, e: &E) {
    log::error!("DataClient-{client_id} failed to send message: {e}");
}
