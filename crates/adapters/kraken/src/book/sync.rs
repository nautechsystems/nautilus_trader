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

use std::{future::Future, sync::Arc};

use ahash::AHashMap;
use nautilus_common::live::dst::time::{self, Duration, Instant};
use nautilus_live::{
    book::{recovery::BookRecovery, snapshot::SnapshotGate, sync::BookSync},
    task::TaskSpawner,
};
use nautilus_model::identifiers::InstrumentId;
use tokio_util::sync::CancellationToken;

use crate::websocket::error::KrakenWsError;

pub(crate) type Book = BookSync<KrakenWsError, i64>;

#[derive(Debug, Default)]
pub(crate) struct BookSyncTracker {
    books: AHashMap<InstrumentId, (u64, Book)>,
    timeout: Duration,
}

impl BookSyncTracker {
    pub(crate) fn new(timeout: Duration) -> Self {
        Self {
            timeout,
            ..Self::default()
        }
    }

    pub(crate) fn track(&mut self, id: InstrumentId, generation: u64, now: Instant) -> &mut Book {
        let timeout = self.timeout;
        let (held, book) = self
            .books
            .entry(id)
            .or_insert_with(|| (generation, Self::waiting(now, timeout)));
        if *held != generation {
            *held = generation;
            *book = Self::waiting(now, timeout);
        }

        book
    }

    fn waiting(now: Instant, timeout: Duration) -> Book {
        let mut book = Book::new(now);
        if !timeout.is_zero() {
            book.arm_deadline(now + timeout);
        }

        book
    }

    pub(crate) fn generation(&self, id: InstrumentId) -> Option<u64> {
        self.books.get(&id).map(|(generation, _)| *generation)
    }

    pub(crate) fn get_mut(&mut self, id: InstrumentId) -> Option<&mut Book> {
        self.books.get_mut(&id).map(|(_, book)| book)
    }

    pub(crate) fn remove(&mut self, id: InstrumentId) {
        self.books.remove(&id);
    }

    pub(crate) fn retain(&mut self, mut held: impl FnMut(InstrumentId) -> bool) {
        self.books.retain(|id, _| held(*id));
    }

    pub(crate) fn clear(&mut self) {
        self.books.clear();
    }

    pub(crate) fn reset_on_reconnect(&mut self, now: Instant) {
        for (_, book) in self.books.values_mut() {
            book.reset_on_reconnect();
            if !self.timeout.is_zero() {
                book.arm_deadline(now + self.timeout);
            }
        }
    }

    pub(crate) fn expired(&mut self, now: Instant) -> Vec<InstrumentId> {
        self.books
            .iter_mut()
            .filter_map(|(id, (_, book))| book.take_expired(now).then_some(*id))
            .collect()
    }
}

pub(crate) fn spawn_recovery<F, Fut>(
    spawner: &TaskSpawner,
    recovery: Arc<BookRecovery<KrakenWsError>>,
    instrument_id: InstrumentId,
    snapshot_timeout: Duration,
    write_timeout: Duration,
    replace: F,
) where
    F: Fn(CancellationToken, SnapshotGate) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<(), KrakenWsError>> + Send,
{
    let guard = recovery.cancellation.clone().drop_guard();
    let result = spawner.spawn(async move {
        let _guard = guard;
        recovery
            .run(
                instrument_id,
                snapshot_timeout,
                |cancel, gate| {
                    let write = replace(cancel, gate);

                    async move {
                        if write_timeout.is_zero() {
                            write.await
                        } else {
                            time::timeout(write_timeout, write)
                                .await
                                .unwrap_or_else(|_| {
                                    Err(KrakenWsError::Timeout(format!(
                                        "Writing {instrument_id} book subscription"
                                    )))
                                })
                        }
                    }
                },
                |e| !matches!(e, KrakenWsError::JsonError(_)),
                KrakenWsError::ConnectionError,
                || KrakenWsError::Timeout(format!("Waiting for {instrument_id} book snapshot")),
            )
            .await;
    });

    if let Err(e) = result {
        log::debug!("Book recovery for {instrument_id} stopped during shutdown: {e}");
    }
}
