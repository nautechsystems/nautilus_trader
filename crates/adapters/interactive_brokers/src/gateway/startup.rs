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

//! Startup flow of the gateway container, apart from the Docker calls it makes.

use std::time::Duration;

use anyhow::Context;

use super::dockerized::ContainerStatus;

pub(super) trait GatewayContainer {
    fn name(&self) -> &str;

    fn is_conflict(error: &anyhow::Error) -> bool;

    async fn status(&self) -> anyhow::Result<ContainerStatus>;

    async fn remove(&self) -> anyhow::Result<()>;

    async fn create(&self) -> anyhow::Result<()>;
}

pub(super) async fn start<C>(container: &C, wait_time: u64) -> anyhow::Result<()>
where
    C: GatewayContainer + Sync,
{
    tracing::debug!("Ensuring gateway is running");

    let status = container.status().await?;

    let broken_statuses = [
        ContainerStatus::NotLoggedIn,
        ContainerStatus::ContainerStopped,
        ContainerStatus::ContainerCreated,
        ContainerStatus::Unknown,
    ];

    match status {
        ContainerStatus::NoContainer => {
            tracing::debug!("No container, starting");
        }
        status if broken_statuses.contains(&status) => {
            tracing::debug!("Status {:?}, removing existing container", status);
            container.remove().await?;
        }
        ContainerStatus::Ready | ContainerStatus::ContainerStarting => {
            tracing::debug!("Status {:?}, using existing container", status);
            return wait_until_ready(container, wait_time).await;
        }
        _ => {}
    }

    tracing::debug!("Starting new container");

    container.create().await?;

    wait_until_ready(container, wait_time).await
}

pub(super) async fn safe_start<C>(container: &C, wait_time: u64) -> anyhow::Result<()>
where
    C: GatewayContainer + Sync,
{
    match start(container, wait_time).await {
        Ok(()) => Ok(()),
        Err(e) if C::is_conflict(&e) => {
            tracing::warn!("Container already exists, continuing");
            wait_until_ready(container, wait_time).await
        }
        Err(e) => Err(e),
    }
}

async fn wait_until_ready<C>(container: &C, wait_time: u64) -> anyhow::Result<()>
where
    C: GatewayContainer + Sync,
{
    tokio::time::timeout(Duration::from_secs(wait_time), async {
        while !matches!(container.status().await, Ok(ContainerStatus::Ready)) {
            tracing::debug!("Waiting for IB Gateway to start");
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    })
    .await
    .with_context(|| {
        format!(
            "gateway `{}` not ready after {wait_time} seconds",
            container.name()
        )
    })?;

    tracing::debug!("Gateway `{}` ready", container.name());
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{
        collections::VecDeque,
        sync::{
            Mutex,
            atomic::{AtomicUsize, Ordering},
        },
    };

    use rstest::rstest;

    use super::*;

    const CONFLICT: &str = "the container name is already in use";

    #[rstest]
    #[tokio::test(start_paused = true)]
    async fn test_start_waits_for_reused_container_to_log_in() {
        let container = TestContainer::new([
            ContainerStatus::ContainerStarting,
            ContainerStatus::ContainerStarting,
            ContainerStatus::Ready,
        ]);
        let started_at = tokio::time::Instant::now();

        start(&container, 5).await.unwrap();

        assert_eq!(container.status_reads.load(Ordering::SeqCst), 3);
        assert_eq!(container.creates.load(Ordering::SeqCst), 0);
        assert_eq!(started_at.elapsed(), Duration::from_secs(1));
    }

    #[rstest]
    #[tokio::test(start_paused = true)]
    async fn test_safe_start_waits_for_login_after_create_conflict() {
        let container = TestContainer::new([
            ContainerStatus::NoContainer,
            ContainerStatus::ContainerStarting,
            ContainerStatus::Ready,
        ]);
        let started_at = tokio::time::Instant::now();

        safe_start(&container, 5).await.unwrap();

        assert_eq!(container.status_reads.load(Ordering::SeqCst), 3);
        assert_eq!(container.creates.load(Ordering::SeqCst), 1);
        assert_eq!(started_at.elapsed(), Duration::from_secs(1));
    }

    #[rstest]
    #[tokio::test(start_paused = true)]
    async fn test_start_times_out_within_wait_on_stalled_status_request() {
        let container = TestContainer::new([ContainerStatus::ContainerStarting]);
        let started_at = tokio::time::Instant::now();

        let error = tokio::time::timeout(Duration::from_secs(10), start(&container, 1))
            .await
            .expect("start outlasted its wait")
            .unwrap_err();

        assert!(
            error.to_string().contains("not ready after 1 seconds"),
            "{error:#}"
        );
        assert_eq!(container.status_reads.load(Ordering::SeqCst), 2);
        assert_eq!(started_at.elapsed(), Duration::from_secs(1));
    }

    /// Answers each status read in turn, then stalls. Creating always hits a conflict.
    struct TestContainer {
        statuses: Mutex<VecDeque<ContainerStatus>>,
        status_reads: AtomicUsize,
        creates: AtomicUsize,
    }

    impl TestContainer {
        fn new<const N: usize>(statuses: [ContainerStatus; N]) -> Self {
            Self {
                statuses: Mutex::new(statuses.into()),
                status_reads: AtomicUsize::new(0),
                creates: AtomicUsize::new(0),
            }
        }
    }

    impl GatewayContainer for TestContainer {
        fn name(&self) -> &'static str {
            "test-gateway"
        }

        fn is_conflict(error: &anyhow::Error) -> bool {
            error.to_string() == CONFLICT
        }

        async fn status(&self) -> anyhow::Result<ContainerStatus> {
            self.status_reads.fetch_add(1, Ordering::SeqCst);
            let status = self.statuses.lock().unwrap().pop_front();

            match status {
                Some(status) => Ok(status),
                None => std::future::pending().await,
            }
        }

        async fn remove(&self) -> anyhow::Result<()> {
            Ok(())
        }

        async fn create(&self) -> anyhow::Result<()> {
            self.creates.fetch_add(1, Ordering::SeqCst);
            anyhow::bail!(CONFLICT)
        }
    }
}
