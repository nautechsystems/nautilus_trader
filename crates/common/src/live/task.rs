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

//! Async task handle storage and lifecycle operations.

use std::sync::Mutex;

use nautilus_core::MUTEX_POISONED;

use super::dst::task::JoinHandle;

/// Stores async task handles without imposing spawn or join policy.
#[derive(Debug, Default)]
pub struct TaskHandles {
    handles: Mutex<Vec<JoinHandle<()>>>,
}

impl TaskHandles {
    /// Stores `handle` after removing handles for completed tasks.
    ///
    /// # Panics
    ///
    /// Panics if the internal mutex is poisoned.
    pub fn push(&self, handle: JoinHandle<()>) {
        let mut handles = self.handles.lock().expect(MUTEX_POISONED);
        handles.retain(|handle| !handle.is_finished());
        handles.push(handle);
    }

    /// Drains and aborts all stored task handles.
    ///
    /// # Panics
    ///
    /// Panics if the internal mutex is poisoned.
    pub fn abort_all(&self) {
        for handle in self.take_all() {
            handle.abort();
        }
    }

    /// Aborts all stored task handles without draining them, so callers can await their
    /// termination later through [`Self::all_finished`].
    ///
    /// # Panics
    ///
    /// Panics if the internal mutex is poisoned.
    pub fn abort_all_retained(&self) {
        for handle in self.handles.lock().expect(MUTEX_POISONED).iter() {
            handle.abort();
        }
    }

    /// Removes and returns all stored task handles.
    ///
    /// # Panics
    ///
    /// Panics if the internal mutex is poisoned.
    #[must_use]
    pub fn take_all(&self) -> Vec<JoinHandle<()>> {
        let mut handles = self.handles.lock().expect(MUTEX_POISONED);
        std::mem::take(&mut *handles)
    }

    /// Returns whether every stored task handle has finished.
    ///
    /// # Panics
    ///
    /// Panics if the internal mutex is poisoned.
    #[must_use]
    pub fn all_finished(&self) -> bool {
        self.handles
            .lock()
            .expect(MUTEX_POISONED)
            .iter()
            .all(JoinHandle::is_finished)
    }

    /// Returns whether no task handles are stored.
    ///
    /// # Panics
    ///
    /// Panics if the internal mutex is poisoned.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.handles.lock().expect(MUTEX_POISONED).is_empty()
    }

    /// Returns the number of stored task handles.
    ///
    /// # Panics
    ///
    /// Panics if the internal mutex is poisoned.
    #[must_use]
    pub fn len(&self) -> usize {
        self.handles.lock().expect(MUTEX_POISONED).len()
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use rstest::rstest;

    use super::*;
    use crate::live::dst::{task, time};

    #[rstest]
    #[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
    #[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
    async fn test_push_prunes_finished_handles() {
        let tasks = TaskHandles::default();

        let finished = task::spawn(async {});

        time::timeout(Duration::from_secs(1), async {
            while !finished.is_finished() {
                task::yield_now().await;
            }
        })
        .await
        .expect("task should finish");

        tasks.push(finished);
        tasks.push(task::spawn(std::future::pending()));

        assert_eq!(tasks.len(), 1);
        tasks.abort_all();
    }

    #[rstest]
    #[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
    #[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
    async fn test_abort_all_drains_before_aborting() {
        let tasks = TaskHandles::default();
        let mut drop_receivers = Vec::new();

        for _ in 0..2 {
            let (drop_tx, drop_rx) = tokio::sync::oneshot::channel();
            let signal = DropSignal { tx: Some(drop_tx) };
            tasks.push(task::spawn(async move {
                let _signal = signal;
                std::future::pending::<()>().await;
            }));
            drop_receivers.push(drop_rx);
        }

        tasks.abort_all();

        assert!(tasks.is_empty());

        for drop_rx in drop_receivers {
            time::timeout(Duration::from_secs(1), drop_rx)
                .await
                .expect("aborted task should drop its future")
                .expect("drop signal should be sent");
        }
    }

    #[rstest]
    #[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
    #[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
    async fn test_abort_all_retained_keeps_finished_handles_observable() {
        let tasks = TaskHandles::default();
        let mut drop_receivers = Vec::new();

        for _ in 0..2 {
            let (drop_tx, drop_rx) = tokio::sync::oneshot::channel();
            let signal = DropSignal { tx: Some(drop_tx) };
            tasks.push(task::spawn(async move {
                let _signal = signal;
                std::future::pending::<()>().await;
            }));
            drop_receivers.push(drop_rx);
        }

        tasks.abort_all_retained();

        assert!(!tasks.is_empty());

        for drop_rx in drop_receivers {
            time::timeout(Duration::from_secs(1), drop_rx)
                .await
                .expect("aborted task should drop its future")
                .expect("drop signal should be sent");
        }

        let _ = time::timeout(Duration::from_secs(1), async {
            while !tasks.all_finished() {
                task::yield_now().await;
            }
        })
        .await;
        assert!(tasks.all_finished());
    }

    #[rstest]
    #[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
    #[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
    async fn test_take_all_extracts_handles() {
        let tasks = TaskHandles::default();
        tasks.push(task::spawn(std::future::pending()));
        tasks.push(task::spawn(std::future::pending()));

        let handles = tasks.take_all();

        assert!(tasks.is_empty());
        assert_eq!(handles.len(), 2);
        for handle in handles {
            handle.abort();
        }
    }

    #[rstest]
    #[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
    #[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
    async fn test_all_finished_preserves_handles() {
        let tasks = TaskHandles::default();
        assert!(tasks.all_finished());

        let (release_tx, release_rx) = tokio::sync::oneshot::channel();
        tasks.push(task::spawn(async move {
            let _ = release_rx.await;
        }));

        assert!(!tasks.all_finished());
        assert_eq!(tasks.len(), 1);

        release_tx.send(()).expect("task should still be waiting");
        time::timeout(Duration::from_secs(1), async {
            while !tasks.all_finished() {
                task::yield_now().await;
            }
        })
        .await
        .expect("task should finish");

        assert!(tasks.all_finished());
        assert_eq!(tasks.len(), 1);
    }

    #[rstest]
    #[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
    #[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
    async fn test_drop_detaches_tasks() {
        let tasks = TaskHandles::default();
        let (release_tx, release_rx) = tokio::sync::oneshot::channel();
        let (done_tx, done_rx) = tokio::sync::oneshot::channel();

        tasks.push(task::spawn(async move {
            let _ = release_rx.await;
            let _ = done_tx.send(());
        }));
        drop(tasks);
        let _ = release_tx.send(());

        time::timeout(Duration::from_secs(1), done_rx)
            .await
            .expect("detached task should complete")
            .expect("completion signal should be sent");
    }

    struct DropSignal {
        tx: Option<tokio::sync::oneshot::Sender<()>>,
    }

    impl Drop for DropSignal {
        fn drop(&mut self) {
            if let Some(tx) = self.tx.take() {
                let _ = tx.send(());
            }
        }
    }
}
