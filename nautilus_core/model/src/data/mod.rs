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
pub mod delta;
pub mod order;
pub mod quote;
pub mod ticker;
pub mod trade;

use nautilus_core::time::UnixNanos;

use self::{bar::Bar, delta::OrderBookDelta, quote::QuoteTick, trade::TradeTick};

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub enum Data {
    Delta(OrderBookDelta),
    Quote(QuoteTick),
    Trade(TradeTick),
    Bar(Bar),
}

pub trait HasTsInit {
    fn get_ts_init(&self) -> UnixNanos;
}

impl HasTsInit for Data {
    fn get_ts_init(&self) -> UnixNanos {
        match self {
            Data::Delta(d) => d.ts_init,
            Data::Quote(q) => q.ts_init,
            Data::Trade(t) => t.ts_init,
            Data::Bar(b) => b.ts_init,
        }
    }
}

impl HasTsInit for OrderBookDelta {
    fn get_ts_init(&self) -> UnixNanos {
        self.ts_init
    }
}

impl HasTsInit for QuoteTick {
    fn get_ts_init(&self) -> UnixNanos {
        self.ts_init
    }
}

impl HasTsInit for TradeTick {
    fn get_ts_init(&self) -> UnixNanos {
        self.ts_init
    }
}

impl HasTsInit for Bar {
    fn get_ts_init(&self) -> UnixNanos {
        self.ts_init
    }
}

pub fn is_monotonically_increasing_by_init<T: HasTsInit>(data: &[T]) -> bool {
    data.windows(2)
        .all(|window| window[0].get_ts_init() <= window[1].get_ts_init())
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

#[no_mangle]
pub extern "C" fn data_clone(data: &Data) -> Data {
    *data
}
