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

//! Test stubs shared by the data and execution tests.

use futures_util::Stream;
use ibapi::{Error, subscriptions::SubscriptionItem};
use nautilus_core::UnixNanos;
use nautilus_model::{
    identifiers::{InstrumentId, Symbol},
    instruments::{CurrencyPair, InstrumentAny},
    types::{Currency, Price, Quantity},
};

/// Channel-backed stand-in for an ibapi subscription; items arrive as data.
pub(crate) struct ChannelSubscription<T>(tokio::sync::mpsc::UnboundedReceiver<Result<T, Error>>);

impl<T> ChannelSubscription<T> {
    pub(crate) const fn new(
        receiver: tokio::sync::mpsc::UnboundedReceiver<Result<T, Error>>,
    ) -> Self {
        Self(receiver)
    }

    pub(crate) fn close(&mut self) {
        self.0.close();
    }
}

impl<T> Stream for ChannelSubscription<T> {
    type Item = Result<SubscriptionItem<T>, Error>;

    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        self.0
            .poll_recv(cx)
            .map(|item| item.map(|result| result.map(SubscriptionItem::Data)))
    }
}

/// Builds a spot currency pair for tests, FX or crypto by its currencies.
pub(crate) fn currency_pair(
    base: &str,
    quote: &str,
    price_precision: u8,
    price_increment: &str,
) -> InstrumentAny {
    let instrument_id = InstrumentId::from(format!("{base}/{quote}.IDEALPRO").as_str());
    let pair = CurrencyPair::builder()
        .instrument_id(instrument_id)
        .raw_symbol(Symbol::from(format!("{base}.{quote}").as_str()))
        .base_currency(Currency::from(base))
        .quote_currency(Currency::from(quote))
        .price_precision(price_precision)
        .size_precision(0)
        .price_increment(Price::from(price_increment))
        .size_increment(Quantity::from("1"))
        .ts_event(UnixNanos::default())
        .ts_init(UnixNanos::default())
        .build()
        .unwrap();
    InstrumentAny::from(pair)
}
