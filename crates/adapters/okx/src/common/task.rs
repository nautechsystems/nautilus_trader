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

use nautilus_common::live::{get_runtime, task::TaskHandles};
use tokio_util::sync::CancellationToken;

const TASK_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(2);

pub(crate) fn spawn_task<F>(tasks: &TaskHandles, cancel: &CancellationToken, fut: F)
where
    F: Future<Output = ()> + Send + 'static,
{
    let cancel = cancel.clone();

    let handle = get_runtime().spawn(async move {
        tokio::select! {
            biased;
            () = cancel.cancelled() => {}
            () = fut => {}
        }
    });
    tasks.push(handle);
}

/// Aborts and drains every task retained by `tasks`.
///
/// Tasks added while a batch is joining are aborted on the next pass. Each batch gets at most two
/// seconds to finish after abort; unfinished handles remain retained so a new client generation
/// cannot start before they terminate.
pub(crate) async fn terminate_tasks(tasks: &TaskHandles, owner: &str) -> anyhow::Result<()> {
    terminate_tasks_with_timeout(tasks, owner, TASK_SHUTDOWN_TIMEOUT).await
}

async fn terminate_tasks_with_timeout(
    tasks: &TaskHandles,
    owner: &str,
    timeout: Duration,
) -> anyhow::Result<()> {
    tasks.abort_all_retained();

    loop {
        let mut handles = tasks.take_all();
        if handles.is_empty() {
            break;
        }

        let join = async {
            for handle in &mut handles {
                if let Err(e) = handle.await
                    && !e.is_cancelled()
                {
                    log::warn!("Error joining {owner} task: {e}");
                }
            }
        };

        if tokio::time::timeout(timeout, join).await.is_err() {
            for handle in handles.into_iter().filter(|handle| !handle.is_finished()) {
                tasks.push(handle);
            }
            tasks.abort_all_retained();
            anyhow::bail!("Timed out joining {owner} tasks after abort");
        }

        tasks.abort_all_retained();
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Condvar, Mutex};

    use super::*;

    #[tokio::test]
    async fn timeout_retains_unfinished_tasks() {
        let tasks = TaskHandles::default();
        let release = Arc::new((Mutex::new(false), Condvar::new()));
        let task_release = Arc::clone(&release);
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();

        tasks.push(tokio::task::spawn_blocking(move || {
            started_tx.send(()).expect("started receiver");
            let (lock, condvar) = &*task_release;
            let released = lock.lock().expect("release mutex");
            drop(
                condvar
                    .wait_while(released, |released| !*released)
                    .expect("release mutex"),
            );
        }));
        started_rx.await.expect("blocking task started");

        let result = terminate_tasks_with_timeout(&tasks, "test", Duration::from_millis(10)).await;
        let retained = tasks.len();

        let (lock, condvar) = &*release;
        *lock.lock().expect("release mutex") = true;
        condvar.notify_all();

        terminate_tasks_with_timeout(&tasks, "test", Duration::from_secs(1))
            .await
            .expect("blocking task terminated");

        assert!(result.is_err());
        assert_eq!(retained, 1);
        assert!(tasks.is_empty());
    }
}
