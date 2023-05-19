// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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

pub mod bar;
pub mod book;
pub mod tick;

use nautilus_core::time::UnixNanos;

use self::{
    bar::Bar,
    book::{OrderBookDelta, OrderBookSnapshot},
    tick::{QuoteTick, TradeTick},
};

#[repr(C)]
#[derive(Debug, Clone)]
pub enum Data {
    Snapshot(OrderBookSnapshot),
    Delta(OrderBookDelta),
    Quote(QuoteTick),
    Trade(TradeTick),
    Bar(Bar),
}

impl Data {
    #[must_use]
    pub fn get_ts_init(&self) -> UnixNanos {
        match self {
            Self::Snapshot(s) => s.ts_init,
            Self::Delta(d) => d.ts_init,
            Self::Quote(q) => q.ts_init,
            Self::Trade(t) => t.ts_init,
            Self::Bar(b) => b.ts_init,
        }
    }
}

impl From<OrderBookSnapshot> for Data {
    fn from(value: OrderBookSnapshot) -> Self {
        Self::Snapshot(value)
    }
}

impl From<OrderBookDelta> for Data {
    fn from(value: OrderBookDelta) -> Self {
        Self::Delta(value)
    }
}

impl From<QuoteTick> for Data {
    fn from(value: QuoteTick) -> Self {
        Self::Quote(value)
    }
}

impl From<TradeTick> for Data {
    fn from(value: TradeTick) -> Self {
        Self::Trade(value)
    }
}

impl From<Bar> for Data {
    fn from(value: Bar) -> Self {
        Self::Bar(value)
    }
}
