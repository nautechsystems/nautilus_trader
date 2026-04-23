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

use std::fmt::Debug;

use nautilus_common::msgbus::{MStr, Topic, TypedHandler};
use nautilus_model::data::{Bar, QuoteTick, TradeTick};

/// Typed subscription for bar aggregator handlers.
///
/// Stores the topic and handler for each data type so we can properly
/// unsubscribe from the typed routers.
#[derive(Clone)]
pub enum BarAggregatorSubscription {
    Bar {
        topic: MStr<Topic>,
        handler: TypedHandler<Bar>,
    },
    Trade {
        topic: MStr<Topic>,
        handler: TypedHandler<TradeTick>,
    },
    Quote {
        topic: MStr<Topic>,
        handler: TypedHandler<QuoteTick>,
    },
}

impl Debug for BarAggregatorSubscription {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Bar { topic, handler } => f
                .debug_struct(stringify!(Bar))
                .field("topic", topic)
                .field("handler_id", &handler.id())
                .finish(),
            Self::Trade { topic, handler } => f
                .debug_struct(stringify!(Trade))
                .field("topic", topic)
                .field("handler_id", &handler.id())
                .finish(),
            Self::Quote { topic, handler } => f
                .debug_struct(stringify!(Quote))
                .field("topic", topic)
                .field("handler_id", &handler.id())
                .finish(),
        }
    }
}
