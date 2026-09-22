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

//! Shared IB API client connection per (host, port, client_id).
//!
//! Data, execution, and historical clients use a single TCP connection per logical
//! connection to avoid client ID conflicts and redundant connections (parity with
//! Python's get_cached_ib_client).

use std::{
    collections::HashMap,
    fmt::Debug,
    future::Future,
    ops::Deref,
    sync::{Arc, LazyLock},
    time::Duration,
};

use anyhow::Context;
use ibapi::{Error, client::Client};
use parking_lot::Mutex;

/// Key for the connection registry: (host, port, client_id).
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct ConnectionKey(String, u16, i32);

enum RegistryEntry<T> {
    Connected {
        client: Arc<T>,
        ref_count: u32,
    },
    Connecting {
        flight: Arc<InFlight<T>>,
        ref_count: u32,
    },
}

struct InFlight<T> {
    result: Mutex<Option<Result<Arc<T>, String>>>,
    notify: tokio::sync::Notify,
}

impl<T> Default for InFlight<T> {
    fn default() -> Self {
        Self {
            result: Mutex::new(None),
            notify: tokio::sync::Notify::new(),
        }
    }
}

impl<T> InFlight<T> {
    fn complete(&self, result: Result<Arc<T>, String>) {
        *self.result.lock() = Some(result);
        self.notify.notify_waiters();
    }

    async fn wait(&self) -> anyhow::Result<Arc<T>> {
        loop {
            let notified = self.notify.notified();

            if let Some(result) = self.result.lock().clone() {
                return result.map_err(anyhow::Error::msg);
            }
            notified.await;
        }
    }
}

struct SharedRegistry<T> {
    entries: Mutex<HashMap<ConnectionKey, RegistryEntry<T>>>,
}

impl<T> Default for SharedRegistry<T> {
    fn default() -> Self {
        Self {
            entries: Mutex::new(HashMap::new()),
        }
    }
}

impl<T> SharedRegistry<T> {
    async fn acquire<Connect, ConnectFuture, IsConnected>(
        &self,
        key: ConnectionKey,
        is_connected: IsConnected,
        connect: Connect,
    ) -> anyhow::Result<Arc<T>>
    where
        Connect: FnOnce() -> ConnectFuture,
        ConnectFuture: Future<Output = anyhow::Result<T>>,
        IsConnected: Fn(&T) -> bool,
    {
        let (flight, should_connect) = {
            let mut entries = self.entries.lock();

            match entries.get_mut(&key) {
                Some(RegistryEntry::Connected { client, ref_count }) if is_connected(client) => {
                    *ref_count = ref_count
                        .checked_add(1)
                        .context("Shared IB client reference count overflow")?;
                    return Ok(Arc::clone(client));
                }
                Some(RegistryEntry::Connecting { flight, .. }) => (Arc::clone(flight), false),
                Some(RegistryEntry::Connected { .. }) | None => {
                    let flight = Arc::new(InFlight::default());
                    entries.insert(
                        key.clone(),
                        RegistryEntry::Connecting {
                            flight: Arc::clone(&flight),
                            ref_count: 1,
                        },
                    );
                    (flight, true)
                }
            }
        };

        if !should_connect {
            let client = flight.wait().await?;
            let mut entries = self.entries.lock();
            match entries.get_mut(&key) {
                Some(RegistryEntry::Connected {
                    client: current,
                    ref_count,
                }) if Arc::ptr_eq(current, &client) => {
                    *ref_count = ref_count
                        .checked_add(1)
                        .context("Shared IB client reference count overflow")?;
                }
                None => {
                    entries.insert(
                        key,
                        RegistryEntry::Connected {
                            client: Arc::clone(&client),
                            ref_count: 1,
                        },
                    );
                }
                _ => anyhow::bail!("Shared IB client was replaced while acquisition was waiting"),
            }
            return Ok(client);
        }

        let mut guard = ConnectGuard {
            registry: self,
            key: key.clone(),
            flight: Arc::clone(&flight),
            armed: true,
        };
        let result = connect().await.map(Arc::new).map_err(|e| format!("{e:#}"));
        {
            let mut entries = self.entries.lock();
            let ref_count = match entries.get(&key) {
                Some(RegistryEntry::Connecting {
                    flight: current,
                    ref_count,
                }) if Arc::ptr_eq(current, &flight) => *ref_count,
                _ => anyhow::bail!("Shared IB client in-flight registry entry was replaced"),
            };

            match &result {
                Ok(client) => {
                    entries.insert(
                        key.clone(),
                        RegistryEntry::Connected {
                            client: Arc::clone(client),
                            ref_count,
                        },
                    );
                }
                Err(_) => {
                    entries.remove(&key);
                }
            }
        }
        flight.complete(result.clone());
        guard.armed = false;
        result.map_err(anyhow::Error::msg)
    }

    fn release(&self, key: &ConnectionKey, client: &Arc<T>) {
        let mut entries = self.entries.lock();
        let Some(RegistryEntry::Connected {
            client: registered,
            ref_count,
        }) = entries.get_mut(key)
        else {
            return;
        };

        if !Arc::ptr_eq(registered, client) {
            return;
        }

        *ref_count = ref_count.saturating_sub(1);
        if *ref_count == 0 {
            entries.remove(key);
        }
    }

    fn ref_count(&self, key: &ConnectionKey) -> Option<u32> {
        match self.entries.lock().get(key) {
            Some(
                RegistryEntry::Connected { ref_count, .. }
                | RegistryEntry::Connecting { ref_count, .. },
            ) => Some(*ref_count),
            None => None,
        }
    }
}

/// Fails the in-flight connect when the winning `acquire` future is dropped mid-connect,
/// so waiters error out and the key can be reacquired instead of hanging forever.
struct ConnectGuard<'a, T> {
    registry: &'a SharedRegistry<T>,
    key: ConnectionKey,
    flight: Arc<InFlight<T>>,
    armed: bool,
}

impl<T> Drop for ConnectGuard<'_, T> {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }

        let mut entries = self.registry.entries.lock();
        if let Some(RegistryEntry::Connecting { flight, .. }) = entries.get(&self.key)
            && Arc::ptr_eq(flight, &self.flight)
        {
            entries.remove(&self.key);
        }
        drop(entries);
        self.flight
            .complete(Err(String::from("Shared IB client connect was cancelled")));
    }
}

/// Global registry: one shared client per (host, port, client_id) with ref count.
static REGISTRY: LazyLock<Arc<SharedRegistry<Client>>> =
    LazyLock::new(|| Arc::new(SharedRegistry::default()));

/// Handle to a shared IB client; when dropped, ref count is decremented and the
/// connection is removed from the registry when the count reaches zero.
pub struct SharedClientHandle {
    client: Arc<Client>,
    registry: Arc<SharedRegistry<Client>>,
    key: ConnectionKey,
}

impl Debug for SharedClientHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(SharedClientHandle))
            .field("key", &self.key)
            .finish_non_exhaustive()
    }
}

impl SharedClientHandle {
    fn new(client: Arc<Client>, registry: Arc<SharedRegistry<Client>>, key: ConnectionKey) -> Self {
        Self {
            client,
            registry,
            key,
        }
    }

    /// Returns a reference to the underlying `Arc<Client>` for call sites that need it.
    pub fn as_arc(&self) -> &Arc<Client> {
        &self.client
    }
}

impl Deref for SharedClientHandle {
    type Target = Client;

    fn deref(&self) -> &Self::Target {
        self.client.as_ref()
    }
}

impl Drop for SharedClientHandle {
    fn drop(&mut self) {
        self.registry.release(&self.key, &self.client);
    }
}

/// Returns a handle to the shared IB client for the given (host, port, client_id).
/// If a connection already exists, its ref count is incremented and the same client
/// is returned. Otherwise a new connection is established and registered.
///
/// # Errors
///
/// Returns an error if connecting to IB Gateway/TWS fails.
pub async fn get_or_connect(
    host: &str,
    port: u16,
    client_id: i32,
    connection_timeout_secs: u64,
) -> anyhow::Result<SharedClientHandle> {
    let key = ConnectionKey(host.to_string(), port, client_id);
    let registry = Arc::clone(&REGISTRY);

    log::debug!(
        "Acquiring shared IB client (host={host}, port={port}, client_id={client_id}, timeout_secs={connection_timeout_secs})"
    );

    let address = format!("{host}:{port}");
    let connect_timeout = Duration::from_secs(connection_timeout_secs);
    let client = registry
        .acquire(key.clone(), Client::is_connected, || async move {
            log::debug!(
                "Establishing shared IB connection to {address} with timeout {connect_timeout:?}"
            );
            tokio::time::timeout(connect_timeout, Client::connect(&address, client_id))
                .await
                .map_err(|_| {
                    anyhow::anyhow!(
                        "Timed out connecting to IB Gateway/TWS after {connection_timeout_secs}s; verify socket API settings and use a TWS/IB Gateway version supported by the IB integration guide"
                    )
                })?
                .map_err(connection_error)
                .context("Failed to connect to IB Gateway/TWS")
        })
        .await?;

    if registry.ref_count(&key).is_some_and(|count| count > 1) {
        log::debug!("Reusing shared IB client (host={host}, port={port}, client_id={client_id})");
    }

    Ok(SharedClientHandle::new(client, registry, key))
}

fn connection_error(e: Error) -> anyhow::Error {
    match e {
        Error::ServerVersion(required, actual, _) => anyhow::anyhow!(
            "TWS/IB Gateway server protocol {actual} is unsupported; server protocol {required} or newer is required. Upgrade TWS/IB Gateway"
        ),
        e => anyhow::Error::new(e),
    }
}

#[cfg(test)]
mod tests {
    use std::{
        sync::atomic::{AtomicUsize, Ordering},
        task::{Context as TaskContext, Waker},
    };

    use rstest::rstest;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    use super::*;

    #[derive(Debug)]
    struct TestClient;

    #[rstest]
    #[tokio::test]
    async fn concurrent_acquire_uses_one_connection_and_reserves_both_references() {
        let registry = Arc::new(SharedRegistry::default());
        let key = ConnectionKey(String::from("127.0.0.1"), 7_497, 42);
        let connect_count = Arc::new(AtomicUsize::new(0));
        let connect_started = Arc::new(tokio::sync::Notify::new());
        let release_connect = Arc::new(tokio::sync::Notify::new());

        let first = {
            let registry = Arc::clone(&registry);
            let key = key.clone();
            let connect_count = Arc::clone(&connect_count);
            let connect_started = Arc::clone(&connect_started);
            let release_connect = Arc::clone(&release_connect);

            tokio::spawn(async move {
                registry
                    .acquire(
                        key,
                        |_| true,
                        || async move {
                            connect_count.fetch_add(1, Ordering::SeqCst);
                            connect_started.notify_one();
                            release_connect.notified().await;
                            Ok::<_, anyhow::Error>(TestClient)
                        },
                    )
                    .await
            })
        };

        connect_started.notified().await;

        let second = registry.acquire(
            key.clone(),
            |_| true,
            || async {
                connect_count.fetch_add(1, Ordering::SeqCst);
                anyhow::bail!("second connector must not run")
            },
        );
        tokio::pin!(second);
        assert!(
            second
                .as_mut()
                .poll(&mut TaskContext::from_waker(Waker::noop()))
                .is_pending()
        );
        assert_eq!(registry.ref_count(&key), Some(1));

        release_connect.notify_one();

        let first_client = first.await.unwrap().unwrap();
        let second_client = second.await.unwrap();

        assert!(Arc::ptr_eq(&first_client, &second_client));
        assert_eq!(connect_count.load(Ordering::SeqCst), 1);
        assert_eq!(registry.ref_count(&key), Some(2));

        registry.release(&key, &first_client);
        assert_eq!(registry.ref_count(&key), Some(1));
        registry.release(&key, &second_client);
        assert_eq!(registry.ref_count(&key), None);
    }

    #[rstest]
    #[tokio::test]
    async fn cancelled_connect_releases_key_and_fails_waiters() {
        let registry = Arc::new(SharedRegistry::default());
        let key = ConnectionKey(String::from("127.0.0.1"), 7_497, 43);
        let connect_started = Arc::new(tokio::sync::Notify::new());

        let winner = {
            let registry = Arc::clone(&registry);
            let key = key.clone();
            let connect_started = Arc::clone(&connect_started);

            tokio::spawn(async move {
                registry
                    .acquire(
                        key,
                        |_| true,
                        || async move {
                            connect_started.notify_one();
                            std::future::pending::<()>().await;
                            Ok::<_, anyhow::Error>(TestClient)
                        },
                    )
                    .await
            })
        };

        connect_started.notified().await;

        let waiter = registry.acquire(
            key.clone(),
            |_| true,
            || async { anyhow::bail!("waiter must not connect while flight is active") },
        );
        tokio::pin!(waiter);
        assert!(
            waiter
                .as_mut()
                .poll(&mut TaskContext::from_waker(Waker::noop()))
                .is_pending()
        );

        winner.abort();
        assert!(winner.await.unwrap_err().is_cancelled());

        let waiter_error = tokio::time::timeout(Duration::from_secs(1), waiter)
            .await
            .expect("waiter must fail fast after cancellation")
            .unwrap_err();
        assert_eq!(
            waiter_error.to_string(),
            "Shared IB client connect was cancelled"
        );
        assert_eq!(registry.ref_count(&key), None);

        let client = registry
            .acquire(
                key.clone(),
                |_| true,
                || async move { Ok::<_, anyhow::Error>(TestClient) },
            )
            .await
            .unwrap();
        assert_eq!(registry.ref_count(&key), Some(1));
        registry.release(&key, &client);
        assert_eq!(registry.ref_count(&key), None);
    }
    #[rstest]
    #[case(1, false)]
    #[case(3, false)]
    #[case(1, true)]
    #[tokio::test]
    async fn cancelled_waiters_do_not_retain_connection(
        #[case] waiter_count: usize,
        #[case] cancel_after_connect: bool,
    ) {
        let registry = SharedRegistry::default();
        let key = ConnectionKey(String::from("127.0.0.1"), 7_497, 44);
        let release_connect = tokio::sync::Notify::new();
        let winner = registry.acquire(
            key.clone(),
            |_| true,
            || async {
                release_connect.notified().await;
                Ok(TestClient)
            },
        );
        tokio::pin!(winner);
        assert!(
            winner
                .as_mut()
                .poll(&mut TaskContext::from_waker(Waker::noop()))
                .is_pending()
        );
        let mut waiters = (0..waiter_count)
            .map(|_| {
                Box::pin(registry.acquire(
                    key.clone(),
                    |_| true,
                    || async { anyhow::bail!("waiter must not connect") },
                ))
            })
            .collect::<Vec<_>>();

        for waiter in &mut waiters {
            assert!(
                waiter
                    .as_mut()
                    .poll(&mut TaskContext::from_waker(Waker::noop()))
                    .is_pending()
            );
        }

        if !cancel_after_connect {
            waiters.clear();
        }
        release_connect.notify_one();
        let client = winner.await.unwrap();
        waiters.clear();
        assert_eq!(registry.ref_count(&key), Some(1));
        registry.release(&key, &client);
        assert_eq!(registry.ref_count(&key), None);
    }

    #[rstest]
    #[tokio::test]
    async fn cancelled_old_waiter_preserves_replacement_connection() {
        let registry = SharedRegistry::default();
        let key = ConnectionKey(String::from("127.0.0.1"), 7_497, 45);
        let mut winner = Box::pin(registry.acquire(
            key.clone(),
            |_| true,
            || async { std::future::pending::<anyhow::Result<TestClient>>().await },
        ));
        assert!(
            winner
                .as_mut()
                .poll(&mut TaskContext::from_waker(Waker::noop()))
                .is_pending()
        );
        let mut waiter = Box::pin(registry.acquire(
            key.clone(),
            |_| true,
            || async { anyhow::bail!("waiter must not connect") },
        ));
        assert!(
            waiter
                .as_mut()
                .poll(&mut TaskContext::from_waker(Waker::noop()))
                .is_pending()
        );
        drop(winner);
        let replacement = registry
            .acquire(key.clone(), |_| true, || async { Ok(TestClient) })
            .await
            .unwrap();
        drop(waiter);
        assert_eq!(registry.ref_count(&key), Some(1));
        registry.release(&key, &replacement);
        assert_eq!(registry.ref_count(&key), None);
    }
    #[rstest]
    #[case(187)]
    #[case(212)]
    #[tokio::test]
    async fn unsupported_server_is_rejected_before_start_api(#[case] server_version: i32) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();

        let server = tokio::spawn(async move {
            for _ in 0..2 {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut magic = [0_u8; 4];
                socket.read_exact(&mut magic).await.unwrap();
                let size = socket.read_u32().await.unwrap();
                let mut version = vec![0_u8; size as usize];
                socket.read_exact(&mut version).await.unwrap();
                let version = String::from_utf8(version).unwrap();
                let response = format!("{server_version}\020260911 12:00:00 UTC\0");
                socket.write_u32(response.len() as u32).await.unwrap();
                socket.write_all(response.as_bytes()).await.unwrap();
                let mut extra = [0_u8; 1];
                let read = tokio::time::timeout(Duration::from_secs(2), socket.read(&mut extra))
                    .await
                    .unwrap()
                    .unwrap();
                assert_eq!(magic, *b"API\0");
                assert_eq!(version.split("..").next(), Some("v213"));
                assert_eq!(
                    read, 0,
                    "unsupported servers must receive no StartApi message"
                );
            }
        });
        let mut errors = Vec::new();
        for _ in 0..2 {
            errors.push(
                get_or_connect("127.0.0.1", port, 917, 1)
                    .await
                    .unwrap_err()
                    .to_string(),
            );
        }
        server.await.unwrap();
        let expected = format!(
            "Failed to connect to IB Gateway/TWS: TWS/IB Gateway server protocol {server_version} is unsupported; server protocol 213 or newer is required. Upgrade TWS/IB Gateway"
        );
        assert_eq!(errors, vec![expected.clone(), expected]);
        assert_eq!(
            REGISTRY.ref_count(&ConnectionKey("127.0.0.1".to_string(), port, 917)),
            None
        );
    }
}
