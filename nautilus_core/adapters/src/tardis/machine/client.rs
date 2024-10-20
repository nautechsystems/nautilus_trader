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

use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

use futures_util::Stream;

use super::{replay_normalized, Error, ReplayNormalizedRequestOptions};
use crate::tardis::machine::enums::{Exchange, WsMessage};

pub type Result<T> = std::result::Result<T, Error>;

pub struct TardisInstrument {
    pub symbol: String,
    pub exchange: Exchange,
}

/// Provides a client for connecting to a [Tardis Machine Server](https://docs.tardis.dev/api/tardis-machine).
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.adapters")
)]
pub struct TardisClient {
    base_url: String,
    replay_signal: Arc<AtomicBool>,
    stream_signals: HashMap<TardisInstrument, Arc<AtomicBool>>,
}

impl TardisClient {
    /// Creates a new [`TardisClient`] instance.
    pub fn new(base_url: impl ToString) -> Self {
        Self {
            base_url: base_url.to_string(),
            replay_signal: Arc::new(AtomicBool::new(false)),
            stream_signals: HashMap::new(),
        }
    }

    pub fn close(&mut self) {
        self.replay_signal.store(true, Ordering::Relaxed);

        for signal in self.stream_signals.values() {
            signal.store(true, Ordering::Relaxed);
        }
        tracing::info!("All signals set to true, shutting down.");
    }

    pub async fn start_replay(
        &self,
        options: Vec<ReplayNormalizedRequestOptions>,
    ) -> Result<impl Stream<Item = Result<WsMessage>>> {
        replay_normalized(&self.base_url, options, self.replay_signal.clone()).await
    }
}
