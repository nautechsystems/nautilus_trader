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

//! Task ownership policy for OKX clients.

use std::{future::Future, time::Duration};

use nautilus_live::task::{TaskGroup, TaskSpawner};

const TASK_GRACEFUL_TIMEOUT: Duration = Duration::from_secs(1);
const TASK_ABORT_TIMEOUT: Duration = Duration::from_secs(2);

pub(crate) fn spawn_task<F>(tasks: &TaskSpawner, fut: F)
where
    F: Future<Output = ()> + Send + 'static,
{
    let cancel = tasks.cancellation_token();

    if let Err(e) = tasks.spawn(async move {
        tokio::select! {
            biased;
            () = cancel.cancelled() => {}
            () = fut => {}
        }
    }) {
        log::debug!("Skipping task spawn after OKX shutdown began: {e}");
    }
}

/// Gracefully completes and then aborts every task retained by `tasks`.
///
/// The scope remains closed if any handle outlives the forced completion bound.
pub(crate) async fn terminate_tasks(tasks: &TaskGroup, owner: &str) -> anyhow::Result<()> {
    tasks.begin_shutdown();
    tasks
        .finish_shutdown(TASK_GRACEFUL_TIMEOUT, TASK_ABORT_TIMEOUT)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to terminate {owner} tasks: {e}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };

    use super::*;

    #[tokio::test]
    async fn termination_cancels_registered_task() {
        let tasks = TaskGroup::new();
        let spawner = tasks.spawner().expect("task spawner");
        let canceled = Arc::new(AtomicBool::new(false));
        let signal = DropSignal(Arc::clone(&canceled));
        spawn_task(&spawner, async move {
            let _signal = signal;
            std::future::pending::<()>().await;
        });

        terminate_tasks(&tasks, "test")
            .await
            .expect("tasks should terminate");

        assert!(canceled.load(Ordering::Acquire));
        assert!(tasks.is_empty());
    }

    struct DropSignal(Arc<AtomicBool>);

    impl Drop for DropSignal {
        fn drop(&mut self) {
            self.0.store(true, Ordering::Release);
        }
    }
}
